//! The display modes an EDID base block names: its detailed timings (the
//! first is the display's preferred mode) and its standard timings, and the
//! display's physical size.
//!
//! Layout: VESA E-EDID 1.3/1.4 as Wikipedia's "Extended Display Identification
//! Data" summarizes it (sections "Structure, version 1.4" and "Detailed Timing
//! Descriptor"; the VESA standard itself is not public):
//! - bytes 0-7: the header `00 FF FF FF FF FF FF 00`;
//! - bytes 18-19: the version and revision;
//! - bytes 21-22: the horizontal and vertical screen size in centimetres;
//! - bytes 38-53: eight standard timings of two bytes, `01 01` unused: the X
//!   resolution `(byte + 31) * 8`, the aspect ratio in bits 7-6 of the second
//!   byte (`00` 16:10 from EDID 1.3, 1:1 before; `01` 4:3, `10` 5:4, `11`
//!   16:9) and the refresh `(bits 5-0) + 60` Hz;
//! - bytes 54-125: four 18-byte descriptors; one whose pixel clock (bytes
//!   0-1) is zero is a display descriptor, not a timing. "For EDID 1.3+ the
//!   preferred timing mode is always in the first Detailed Timing Descriptor";
//! - byte 127: the checksum, "Sum of all 128 bytes should equal 0 (mod 256)".
//!
//! A detailed timing: the pixel clock in 10 kHz units, little-endian (bytes
//! 0-1); the horizontal active and blanking pixels, 8 low bits each in bytes
//! 2 and 3 and their 4 high bits in the upper and lower nibble of byte 4; the
//! vertical ones likewise in bytes 5, 6 and 7; the image size in mm, 8 low
//! bits in bytes 12 and 13 and 4 high bits in the nibbles of byte 14; bit 7 of
//! byte 17 set for an interlaced signal.
//!
//! The established timings (bytes 35-37) are not read: the source above does
//! not give their table.

/// Bytes of an EDID block.
pub const BLOCK_BYTES: usize = 128;

/// Most modes [`Modes`] holds.
pub const MAX_MODES: usize = 16;

/// The header every EDID base block starts with.
const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

/// Where the version and revision bytes are.
const VERSION: usize = 18;
const REVISION: usize = 19;

/// Where the screen size in centimetres is.
const SCREEN_SIZE_CM: usize = 21;

/// A detailed timing's image size that names no size: both 8-bit fields all
/// ones and no high bits ([`Edid::image_size_mm`]).
const PLACEHOLDER_MM: (u32, u32) = (255, 255);

/// Where the standard timings start, and how many there are.
const STANDARD_TIMINGS: usize = 38;
const STANDARD_TIMING_COUNT: usize = 8;

/// Where the descriptors start, how long each is, and how many there are.
const DESCRIPTORS: usize = 54;
const DESCRIPTOR_BYTES: usize = 18;
const DESCRIPTOR_COUNT: usize = 4;

/// One display mode: its size in pixels and its refresh rate in millihertz
/// (0 when unknown).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
}

/// Why a block is not an EDID base block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// It does not start with the EDID header.
    Header,
    /// Its bytes do not sum to 0 modulo 256.
    Checksum,
}

/// A checked EDID base block.
#[derive(Clone, Copy, Debug)]
pub struct Edid<'block> {
    bytes: &'block [u8; BLOCK_BYTES],
}

impl<'block> Edid<'block> {
    /// `bytes` as an EDID base block, if its header and checksum are right.
    pub fn new(bytes: &'block [u8; BLOCK_BYTES]) -> Result<Self, Invalid> {
        if bytes[..HEADER.len()] != HEADER {
            return Err(Invalid::Header);
        }
        if bytes.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte)) != 0 {
            return Err(Invalid::Checksum);
        }
        Ok(Self { bytes })
    }

    /// The EDID version and revision (1.4 is `(1, 4)`).
    #[must_use]
    pub const fn version(&self) -> (u8, u8) {
        (self.bytes[VERSION], self.bytes[REVISION])
    }

    /// The descriptor `index` (0 to 3).
    fn descriptor(self, index: usize) -> &'block [u8; DESCRIPTOR_BYTES] {
        let start = DESCRIPTORS + index * DESCRIPTOR_BYTES;
        self.bytes[start..start + DESCRIPTOR_BYTES]
            .try_into()
            .unwrap_or(&[0; DESCRIPTOR_BYTES])
    }

    /// The detailed timings, the preferred one first: every descriptor that
    /// is a timing (nonzero pixel clock) of a progressive signal.
    pub fn detailed_timings(&self) -> impl Iterator<Item = Timing> + 'block {
        let edid = *self;
        (0..DESCRIPTOR_COUNT).filter_map(move |index| detailed_timing(edid.descriptor(index)))
    }

    /// The standard timings, in the order the block lists them.
    pub fn standard_timings(&self) -> impl Iterator<Item = Timing> + 'block {
        let edid = *self;
        (0..STANDARD_TIMING_COUNT).filter_map(move |index| {
            let at = STANDARD_TIMINGS + index * 2;
            standard_timing(
                [edid.bytes[at], edid.bytes[at + 1]],
                edid.version() >= (1, 3),
            )
        })
    }

    /// The display's image size in millimetres: the first detailed timing's
    /// (to the millimetre), else the screen size in centimetres; `None` when
    /// neither is given. A timing size of exactly 255x255 mm counts as not
    /// given: a display that fills the bytes with all ones names no size (a
    /// 165x95 mm 1024x600 panel's EDID says 255x255 mm by its timing and
    /// 18x13 cm by its screen size bytes; maintainer decision, 2026-11-25).
    #[must_use]
    pub fn image_size_mm(&self) -> Option<(u32, u32)> {
        self.timing_size_mm()
            .filter(|&size| size != PLACEHOLDER_MM)
            .or(self.screen_size_mm())
    }

    /// The image size the first detailed timing gives, in millimetres, if
    /// both are nonzero.
    #[must_use]
    pub fn timing_size_mm(&self) -> Option<(u32, u32)> {
        (0..DESCRIPTOR_COUNT)
            .map(|index| self.descriptor(index))
            .find(|descriptor| pixel_clock(descriptor) != 0)
            .map(|descriptor| {
                (
                    u32::from(descriptor[12]) | u32::from(descriptor[14] >> 4) << 8,
                    u32::from(descriptor[13]) | u32::from(descriptor[14] & 0xf) << 8,
                )
            })
            .filter(|&(width, height)| width != 0 && height != 0)
    }

    /// The screen size bytes 21-22 give, in millimetres (whole centimetres),
    /// if both are nonzero: with one of them zero they are an aspect ratio.
    #[must_use]
    pub fn screen_size_mm(&self) -> Option<(u32, u32)> {
        match (self.bytes[SCREEN_SIZE_CM], self.bytes[SCREEN_SIZE_CM + 1]) {
            (0, _) | (_, 0) => None,
            (width, height) => Some((u32::from(width) * 10, u32::from(height) * 10)),
        }
    }

    /// The modes the block names, preferred first: the detailed timings, then
    /// the standard timings, each size once (the first refresh rate it was
    /// named with), at most [`MAX_MODES`].
    #[must_use]
    pub fn modes(&self) -> Modes {
        let mut modes = Modes::empty();
        for timing in self.detailed_timings().chain(self.standard_timings()) {
            modes.push(timing);
        }
        modes
    }
}

/// A descriptor's pixel clock in 10 kHz units.
const fn pixel_clock(descriptor: &[u8; DESCRIPTOR_BYTES]) -> u16 {
    u16::from_le_bytes([descriptor[0], descriptor[1]])
}

/// The timing a detailed timing descriptor names, if it is one of a
/// progressive signal.
fn detailed_timing(descriptor: &[u8; DESCRIPTOR_BYTES]) -> Option<Timing> {
    let clock = pixel_clock(descriptor);
    let interlaced = descriptor[17] & 0x80 != 0;
    if clock == 0 || interlaced {
        return None;
    }
    let twelve_bits = |low: u8, high_nibble: u8| u32::from(low) | u32::from(high_nibble) << 8;
    let width = twelve_bits(descriptor[2], descriptor[4] >> 4);
    let horizontal_blank = twelve_bits(descriptor[3], descriptor[4] & 0xf);
    let height = twelve_bits(descriptor[5], descriptor[7] >> 4);
    let vertical_blank = twelve_bits(descriptor[6], descriptor[7] & 0xf);
    if width == 0 || height == 0 {
        return None;
    }
    let total = u64::from(width + horizontal_blank) * u64::from(height + vertical_blank);
    let clock_hz = u64::from(clock) * 10_000;
    let refresh_mhz = u32::try_from(clock_hz * 1000 / total).unwrap_or(0);
    Some(Timing {
        width,
        height,
        refresh_mhz,
    })
}

/// The timing a standard timing's two bytes name, if any: `00` in the first
/// byte is reserved and `01 01` unused. `revision_1_3` reads aspect ratio `00`
/// as 16:10 (EDID 1.3 and later) instead of 1:1.
fn standard_timing(bytes: [u8; 2], revision_1_3: bool) -> Option<Timing> {
    if bytes[0] == 0 || bytes == [0x01, 0x01] {
        return None;
    }
    let width = (u32::from(bytes[0]) + 31) * 8;
    let (across, down) = match bytes[1] >> 6 {
        0 if revision_1_3 => (16, 10),
        0 => (1, 1),
        1 => (4, 3),
        2 => (5, 4),
        _ => (16, 9),
    };
    Some(Timing {
        width,
        height: width * down / across,
        refresh_mhz: (u32::from(bytes[1] & 0x3f) + 60) * 1000,
    })
}

/// A list of modes, each size at most once, in the order they were pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modes {
    modes: [Timing; MAX_MODES],
    count: usize,
}

impl Modes {
    /// No modes.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            modes: [Timing {
                width: 0,
                height: 0,
                refresh_mhz: 0,
            }; MAX_MODES],
            count: 0,
        }
    }

    /// The one mode an output without an EDID gets.
    #[must_use]
    pub const fn fallback(mode: Timing) -> Self {
        let mut modes = Self::empty();
        modes.modes[0] = mode;
        modes.count = 1;
        modes
    }

    /// Add `mode` unless its size is listed already or the list is full;
    /// returns whether it was added.
    pub fn push(&mut self, mode: Timing) -> bool {
        let listed = self
            .as_slice()
            .iter()
            .any(|known| (known.width, known.height) == (mode.width, mode.height));
        if listed || self.count == MAX_MODES {
            return false;
        }
        self.modes[self.count] = mode;
        self.count += 1;
        true
    }

    /// The modes.
    #[must_use]
    pub fn as_slice(&self) -> &[Timing] {
        &self.modes[..self.count]
    }
}
