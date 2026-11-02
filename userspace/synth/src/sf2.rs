//! Zero-copy reader of a SoundFont 2 file (SoundFont 2.04 specification,
//! sections 4 to 7).
//!
//! [`SoundFont::parse`] checks the RIFF structure and the record sizes and
//! keeps slices of the file: the `INFO` list, the 16-bit sample data and the
//! nine `pdta` record arrays. Records are decoded on access, little-endian,
//! from those slices; nothing is copied.

/// Size in bytes of each `pdta` record kind.
pub const PRESET_HEADER_SIZE: usize = 38;
/// Size of a `pbag` or `ibag` record.
pub const BAG_SIZE: usize = 4;
/// Size of a `pmod` or `imod` record.
pub const MODULATOR_SIZE: usize = 10;
/// Size of a `pgen` or `igen` record.
pub const GENERATOR_SIZE: usize = 4;
/// Size of an `inst` record.
pub const INSTRUMENT_HEADER_SIZE: usize = 22;
/// Size of an `shdr` record.
pub const SAMPLE_HEADER_SIZE: usize = 46;

/// Why a file is not a usable SoundFont.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sf2Error {
    /// Not a `RIFF` file of form `sfbk`.
    NotSoundFont,
    /// A chunk's size runs past its parent.
    Truncated,
    /// A required chunk is missing (its four-character code).
    MissingChunk([u8; 4]),
    /// A record chunk's size is not a whole number of records, or it lacks
    /// its terminal record (its four-character code).
    BadRecords([u8; 4]),
    /// Indices between the record arrays are out of range or decreasing.
    BadIndex,
}

/// The parts of a SoundFont file, borrowed from its bytes.
#[derive(Clone, Copy, Debug)]
pub struct SoundFont<'a> {
    /// The body of the `INFO` list, after the `INFO` form type.
    pub info: &'a [u8],
    /// The `smpl` chunk: 16-bit little-endian sample points.
    pub sample_data: &'a [u8],
    /// `phdr` records, the terminal `EOP` included.
    pub preset_headers: &'a [u8],
    /// `pbag` records.
    pub preset_bags: &'a [u8],
    /// `pmod` records.
    pub preset_modulators: &'a [u8],
    /// `pgen` records.
    pub preset_generators: &'a [u8],
    /// `inst` records, the terminal `EOI` included.
    pub instrument_headers: &'a [u8],
    /// `ibag` records.
    pub instrument_bags: &'a [u8],
    /// `imod` records.
    pub instrument_modulators: &'a [u8],
    /// `igen` records.
    pub instrument_generators: &'a [u8],
    /// `shdr` records, the terminal `EOS` included.
    pub sample_headers: &'a [u8],
}

/// A `phdr` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PresetHeader {
    /// Name, NUL padded.
    pub name: [u8; 20],
    /// MIDI program, 0 to 127.
    pub program: u16,
    /// MIDI bank, 128 for percussion.
    pub bank: u16,
    /// Index of the preset's first `pbag` record.
    pub bag_index: u16,
}

/// A `pbag` or `ibag` record: where a zone's generators and modulators start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bag {
    /// Index of the zone's first generator.
    pub generator_index: u16,
    /// Index of the zone's first modulator.
    pub modulator_index: u16,
}

/// A `pgen` or `igen` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Generator {
    /// Which generator (section 8.1.2), see [`crate::generator`].
    pub operator: u16,
    /// Its amount, raw: a signed word, an unsigned word or a byte range.
    pub amount: u16,
}

impl Generator {
    /// The amount as a signed word.
    #[must_use]
    pub const fn signed(self) -> i16 {
        self.amount.cast_signed()
    }

    /// The amount as a range `(low, high)` (`keyRange`, `velRange`).
    #[must_use]
    pub const fn range(self) -> (u8, u8) {
        let [low, high] = self.amount.to_le_bytes();
        (low, high)
    }
}

/// An `inst` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstrumentHeader {
    /// Name, NUL padded.
    pub name: [u8; 20],
    /// Index of the instrument's first `ibag` record.
    pub bag_index: u16,
}

/// An `shdr` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleHeader {
    /// Name, NUL padded.
    pub name: [u8; 20],
    /// First sample point, an index into the sample data.
    pub start: u32,
    /// One past the last sample point.
    pub end: u32,
    /// First point of the loop.
    pub loop_start: u32,
    /// One past the last point of the loop.
    pub loop_end: u32,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// MIDI key of the recorded pitch.
    pub original_pitch: u8,
    /// Pitch correction in cents.
    pub pitch_correction: i8,
    /// Index of the other side of a stereo pair.
    pub sample_link: u16,
    /// 1 mono, 2 right, 4 left, 8 linked; `0x8000` set for ROM samples.
    pub sample_type: u16,
}

/// Sample type of a mono sample.
pub const SAMPLE_TYPE_MONO: u16 = 1;

const fn word(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

const fn dword(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn name(bytes: &[u8]) -> [u8; 20] {
    let mut name = [0; 20];
    name.copy_from_slice(&bytes[..20]);
    name
}

/// A RIFF four-character code.
type FourCc = [u8; 4];

/// A chunk: its code, its body, and the bytes after it.
struct Chunk<'a> {
    code: FourCc,
    body: &'a [u8],
    rest: &'a [u8],
}

/// The four bytes at `offset`.
const fn four_cc(bytes: &[u8], offset: usize) -> FourCc {
    dword(bytes, offset).to_le_bytes()
}

/// The chunk at the start of `bytes`.
fn chunk(bytes: &[u8]) -> Result<Chunk<'_>, Sf2Error> {
    if bytes.len() < 8 {
        return Err(Sf2Error::Truncated);
    }
    let size = usize::try_from(dword(bytes, 4))
        .ok()
        .ok_or(Sf2Error::Truncated)?;
    let body = bytes.get(8..8 + size).ok_or(Sf2Error::Truncated)?;
    // Chunks are padded to an even size.
    let next = (8 + size + (size & 1)).min(bytes.len());
    Ok(Chunk {
        code: four_cc(bytes, 0),
        body,
        rest: &bytes[next..],
    })
}

/// The body of the `LIST` chunk of form `form` among `chunks`.
fn list(mut chunks: &[u8], form: FourCc) -> Result<&[u8], Sf2Error> {
    while !chunks.is_empty() {
        let found = chunk(chunks)?;
        if found.code == *b"LIST" && found.body.len() >= 4 && four_cc(found.body, 0) == form {
            return Ok(&found.body[4..]);
        }
        chunks = found.rest;
    }
    Err(Sf2Error::MissingChunk(form))
}

/// The body of the sub-chunk `code` among `chunks`.
fn sub_chunk(mut chunks: &[u8], code: FourCc) -> Result<&[u8], Sf2Error> {
    while !chunks.is_empty() {
        let found = chunk(chunks)?;
        if found.code == code {
            return Ok(found.body);
        }
        chunks = found.rest;
    }
    Err(Sf2Error::MissingChunk(code))
}

/// A record array of `size`-byte records with at least `minimum` records.
fn records(chunks: &[u8], code: FourCc, size: usize, minimum: usize) -> Result<&[u8], Sf2Error> {
    let body = sub_chunk(chunks, code)?;
    if !body.len().is_multiple_of(size) || body.len() / size < minimum {
        return Err(Sf2Error::BadRecords(code));
    }
    Ok(body)
}

impl<'a> SoundFont<'a> {
    /// Read the structure of the SoundFont `bytes`.
    ///
    /// # Errors
    ///
    /// [`Sf2Error`] when the RIFF structure, a record array or the indices
    /// between the arrays are malformed.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Sf2Error> {
        let riff = chunk(bytes)?;
        if riff.code != *b"RIFF" || riff.body.len() < 4 || four_cc(riff.body, 0) != *b"sfbk" {
            return Err(Sf2Error::NotSoundFont);
        }
        let chunks = &riff.body[4..];
        let info = list(chunks, *b"INFO")?;
        let sample_data = sub_chunk(list(chunks, *b"sdta")?, *b"smpl")?;
        let pdta = list(chunks, *b"pdta")?;
        let font = Self {
            info,
            sample_data,
            preset_headers: records(pdta, *b"phdr", PRESET_HEADER_SIZE, 2)?,
            preset_bags: records(pdta, *b"pbag", BAG_SIZE, 1)?,
            preset_modulators: records(pdta, *b"pmod", MODULATOR_SIZE, 1)?,
            preset_generators: records(pdta, *b"pgen", GENERATOR_SIZE, 1)?,
            instrument_headers: records(pdta, *b"inst", INSTRUMENT_HEADER_SIZE, 2)?,
            instrument_bags: records(pdta, *b"ibag", BAG_SIZE, 1)?,
            instrument_modulators: records(pdta, *b"imod", MODULATOR_SIZE, 1)?,
            instrument_generators: records(pdta, *b"igen", GENERATOR_SIZE, 1)?,
            sample_headers: records(pdta, *b"shdr", SAMPLE_HEADER_SIZE, 1)?,
        };
        font.check_indices()?;
        Ok(font)
    }

    /// Every bag index and generator/modulator index is non-decreasing and
    /// within its target array (the terminal records included).
    fn check_indices(&self) -> Result<(), Sf2Error> {
        let bag_runs_ok = |headers: usize, bag_of: &dyn Fn(usize) -> u16, bags: usize| {
            (0..headers).all(|index| {
                let bag = usize::from(bag_of(index));
                bag < bags && (index == 0 || usize::from(bag_of(index - 1)) <= bag)
            })
        };
        let zone_runs_ok = |bags: &[u8], generators: usize, modulators: usize| {
            let count = bags.len() / BAG_SIZE;
            (0..count).all(|index| {
                let bag = bag_at(bags, index);
                usize::from(bag.generator_index) < generators
                    && usize::from(bag.modulator_index) < modulators
                    && (index == 0 || {
                        let previous = bag_at(bags, index - 1);
                        previous.generator_index <= bag.generator_index
                            && previous.modulator_index <= bag.modulator_index
                    })
            })
        };
        let ok = bag_runs_ok(
            self.preset_headers.len() / PRESET_HEADER_SIZE,
            &|index| self.preset_header(index).bag_index,
            self.preset_bags.len() / BAG_SIZE,
        ) && bag_runs_ok(
            self.instrument_headers.len() / INSTRUMENT_HEADER_SIZE,
            &|index| self.instrument_header(index).bag_index,
            self.instrument_bags.len() / BAG_SIZE,
        ) && zone_runs_ok(
            self.preset_bags,
            self.preset_generators.len() / GENERATOR_SIZE,
            self.preset_modulators.len() / MODULATOR_SIZE,
        ) && zone_runs_ok(
            self.instrument_bags,
            self.instrument_generators.len() / GENERATOR_SIZE,
            self.instrument_modulators.len() / MODULATOR_SIZE,
        );
        if ok { Ok(()) } else { Err(Sf2Error::BadIndex) }
    }

    /// Number of presets, not counting the terminal record.
    #[must_use]
    pub const fn preset_count(&self) -> usize {
        self.preset_headers.len() / PRESET_HEADER_SIZE - 1
    }

    /// Number of instruments, not counting the terminal record.
    #[must_use]
    pub const fn instrument_count(&self) -> usize {
        self.instrument_headers.len() / INSTRUMENT_HEADER_SIZE - 1
    }

    /// Number of samples, not counting the terminal record.
    #[must_use]
    pub const fn sample_count(&self) -> usize {
        self.sample_headers.len() / SAMPLE_HEADER_SIZE - 1
    }

    /// Number of 16-bit sample points.
    #[must_use]
    pub const fn sample_points(&self) -> usize {
        self.sample_data.len() / 2
    }

    /// Preset header `index` (the terminal record is `preset_count()`).
    #[must_use]
    pub fn preset_header(&self, index: usize) -> PresetHeader {
        let record = &self.preset_headers[index * PRESET_HEADER_SIZE..][..PRESET_HEADER_SIZE];
        PresetHeader {
            name: name(record),
            program: word(record, 20),
            bank: word(record, 22),
            bag_index: word(record, 24),
        }
    }

    /// Preset bag `index`.
    #[must_use]
    pub fn preset_bag(&self, index: usize) -> Bag {
        bag_at(self.preset_bags, index)
    }

    /// Preset generator `index`.
    #[must_use]
    pub fn preset_generator(&self, index: usize) -> Generator {
        generator_at(self.preset_generators, index)
    }

    /// Instrument header `index` (the terminal record is
    /// `instrument_count()`).
    #[must_use]
    pub fn instrument_header(&self, index: usize) -> InstrumentHeader {
        let record =
            &self.instrument_headers[index * INSTRUMENT_HEADER_SIZE..][..INSTRUMENT_HEADER_SIZE];
        InstrumentHeader {
            name: name(record),
            bag_index: word(record, 20),
        }
    }

    /// Instrument bag `index`.
    #[must_use]
    pub fn instrument_bag(&self, index: usize) -> Bag {
        bag_at(self.instrument_bags, index)
    }

    /// Instrument generator `index`.
    #[must_use]
    pub fn instrument_generator(&self, index: usize) -> Generator {
        generator_at(self.instrument_generators, index)
    }

    /// Sample header `index` (the terminal record is `sample_count()`).
    #[must_use]
    pub fn sample_header(&self, index: usize) -> SampleHeader {
        let record = &self.sample_headers[index * SAMPLE_HEADER_SIZE..][..SAMPLE_HEADER_SIZE];
        SampleHeader {
            name: name(record),
            start: dword(record, 20),
            end: dword(record, 24),
            loop_start: dword(record, 28),
            loop_end: dword(record, 32),
            sample_rate: dword(record, 36),
            original_pitch: word(record, 40).to_le_bytes()[0],
            pitch_correction: word(record, 40).to_le_bytes()[1].cast_signed(),
            sample_link: word(record, 42),
            sample_type: word(record, 44),
        }
    }

    /// Sample point `index` of the sample data, 0 past its end.
    #[must_use]
    pub fn sample_point(&self, index: u32) -> i16 {
        let Ok(index) = usize::try_from(index) else {
            return 0;
        };
        match self.sample_data.get(index * 2..index * 2 + 2) {
            Some(&[low, high]) => i16::from_le_bytes([low, high]),
            _ => 0,
        }
    }
}

fn bag_at(bags: &[u8], index: usize) -> Bag {
    let record = &bags[index * BAG_SIZE..][..BAG_SIZE];
    Bag {
        generator_index: word(record, 0),
        modulator_index: word(record, 2),
    }
}

fn generator_at(generators: &[u8], index: usize) -> Generator {
    let record = &generators[index * GENERATOR_SIZE..][..GENERATOR_SIZE];
    Generator {
        operator: word(record, 0),
        amount: word(record, 2),
    }
}
