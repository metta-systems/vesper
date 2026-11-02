#![no_std]

//! The property buffers that configure the Raspberry Pi's framebuffer, and
//! the reading of the firmware's answers.
//!
//! Two requests set the display up. The first sets the physical and virtual
//! size and the depth and allocates the buffer; the firmware answers with its
//! bus address and size. The second sets the alpha mode to ignored and reads
//! the pixel order and the pitch.

use mailbox_impl::tag;

/// Words of the configuring request and of its answer.
pub const CONFIGURE_WORDS: usize = 22;

/// Words of the attributes request and of its answer.
pub const ATTRIBUTE_WORDS: usize = 16;

/// Bit 31 of a tag's request/response word: set once the firmware answered.
const TAG_ANSWERED: u32 = 0x8000_0000;

/// Alignment the framebuffer is allocated with, in bytes.
const ALIGNMENT: u32 = 16;

/// The alpha mode that ignores the alpha channel.
const ALPHA_IGNORED: u32 = 2;

/// The request that sets the display up and allocates the buffer.
pub fn configure_request(width: u32, height: u32, depth: u32) -> [u32; CONFIGURE_WORDS] {
    let mut words = [0; CONFIGURE_WORDS];
    words[0] = byte_size(CONFIGURE_WORDS);
    // Tags: id, value bytes, request code, values.
    words[2..7].copy_from_slice(&[tag::SET_PHYSICAL_SIZE, 8, 0, width, height]);
    words[7..12].copy_from_slice(&[tag::SET_VIRTUAL_SIZE, 8, 0, width, height]);
    words[12..16].copy_from_slice(&[tag::SET_DEPTH, 4, 0, depth]);
    words[16..21].copy_from_slice(&[tag::ALLOCATE_BUFFER, 8, 0, ALIGNMENT, 0]);
    // Word 21 is the end tag.
    words
}

/// Where the firmware put the framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    /// Address of the buffer on the `VideoCore` bus
    pub bus_address: u32,
    /// Size of the buffer in bytes
    pub bytes: u32,
}

/// The allocation the firmware answered `answer` with, or the tag's code word
/// when it did not answer the allocation.
pub fn read_allocation(answer: &[u32; CONFIGURE_WORDS]) -> Result<Allocation, u32> {
    if answer[18] & TAG_ANSWERED == 0 {
        return Err(answer[18]);
    }
    Ok(Allocation {
        bus_address: answer[19],
        bytes: answer[20],
    })
}

/// The request that ignores alpha and reads the pixel order and the pitch.
pub fn attributes_request() -> [u32; ATTRIBUTE_WORDS] {
    let mut words = [0; ATTRIBUTE_WORDS];
    words[0] = byte_size(ATTRIBUTE_WORDS);
    words[2..6].copy_from_slice(&[tag::GET_PIXEL_ORDER, 4, 0, 0]);
    words[6..10].copy_from_slice(&[tag::SET_ALPHA_MODE, 4, 0, ALPHA_IGNORED]);
    words[10..14].copy_from_slice(&[tag::GET_PITCH, 4, 0, 0]);
    // Word 14 is the end tag, word 15 padding.
    words
}

/// What the firmware reported about the pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attributes {
    /// The pixel order: 0 blue-green-red, 1 red-green-blue
    pub pixel_order: u32,
    /// Bytes from the start of one row to the next
    pub pitch: u32,
}

/// The attributes the firmware answered `answer` with, or the code word of the
/// tag it did not answer.
pub fn read_attributes(answer: &[u32; ATTRIBUTE_WORDS]) -> Result<Attributes, u32> {
    for code in [answer[4], answer[12]] {
        if code & TAG_ANSWERED == 0 {
            return Err(code);
        }
    }
    Ok(Attributes {
        pixel_order: answer[5],
        pitch: answer[13],
    })
}

/// The physical address of the bus address `bus`, given the offset between
/// the two. Without a usable offset the bus aliases (the top two address bits)
/// are masked off.
pub fn physical_address(bus: u32, bus_offset: u64) -> u64 {
    u64::from(bus)
        .checked_sub(bus_offset)
        .unwrap_or_else(|| u64::from(bus) & 0x3fff_ffff)
}

fn byte_size(words: usize) -> u32 {
    u32::try_from(words * 4).unwrap_or(u32::MAX)
}

/// Whether the bytes `[start, start + length)` lie in the memory the blocks
/// `[base, end)` make up together; the blocks may be adjacent.
pub fn covered_by(blocks: &[(u64, u64)], start: u64, length: u64) -> bool {
    let Some(end) = start.checked_add(length) else {
        return false;
    };
    let mut cursor = start;
    while cursor < end {
        let Some(&(_, block_end)) = blocks
            .iter()
            .find(|&&(base, block_end)| base <= cursor && cursor < block_end)
        else {
            return false;
        };
        cursor = block_end;
    }
    true
}

/// Bytes of a page: video memory is handed out in whole pages.
pub const PAGE_BYTES: u64 = 4096;

/// `size_bits` of a 4 KiB Frame.
pub const PAGE_FRAME_BITS: u8 = 12;

/// `size_bits` of a 2 MiB Frame.
pub const LARGE_FRAME_BITS: u8 = 21;

/// Most objects one Retype creates.
pub const MAX_RETYPE_COUNT: u32 = 256;

/// The pages the bytes `[address, address + bytes)` occupy, as the range
/// `[start, end)` of whole pages, or `None` if it is empty or overflows.
pub fn page_range(address: u64, bytes: u64) -> Option<(u64, u64)> {
    let end = address.checked_add(bytes)?;
    if bytes == 0 {
        return None;
    }
    Some((
        address & !(PAGE_BYTES - 1),
        end.checked_next_multiple_of(PAGE_BYTES)?,
    ))
}

/// A run of consecutive Frames of one size, which one Retype from one block
/// of video memory carves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRun {
    /// Physical address of the first Frame
    pub address: u64,
    /// `size_bits` of each Frame: [`PAGE_FRAME_BITS`] or [`LARGE_FRAME_BITS`]
    pub size_bits: u8,
    /// How many Frames, at most [`MAX_RETYPE_COUNT`]
    pub count: u32,
}

impl FrameRun {
    /// Bytes the run covers.
    pub const fn bytes(self) -> u64 {
        (self.count as u64) << self.size_bits
    }

    /// Address after the run's last Frame.
    pub const fn end(self) -> u64 {
        self.address + self.bytes()
    }
}

/// The runs of Frames that cover the pages `[start, end)` of video memory made
/// up of `blocks` (each `(base, end)`), in address order.
///
/// A 2 MiB Frame is used wherever one lies fully inside the range, on its own
/// alignment and inside a single block; every other page is a 4 KiB Frame. A
/// run never leaves its block, since one Retype carves from one block. The
/// plan ends early at memory no block holds, so check the range with
/// [`covered_by`] first.
#[derive(Clone, Debug)]
pub struct FramePlan<'blocks> {
    blocks: &'blocks [(u64, u64)],
    cursor: u64,
    end: u64,
}

impl<'blocks> FramePlan<'blocks> {
    /// The plan covering `[start, end)`, both multiples of [`PAGE_BYTES`].
    pub const fn new(blocks: &'blocks [(u64, u64)], start: u64, end: u64) -> Self {
        Self {
            blocks,
            cursor: start,
            end,
        }
    }

    /// The end of the block holding `address`.
    fn block_end(&self, address: u64) -> Option<u64> {
        self.blocks
            .iter()
            .find(|&&(base, block_end)| base <= address && address < block_end)
            .map(|&(_, block_end)| block_end)
    }

    /// Whether a 2 MiB Frame can start at `address`.
    fn large_fits(&self, address: u64) -> bool {
        let large = 1_u64 << LARGE_FRAME_BITS;
        address.is_multiple_of(large)
            && address
                .checked_add(large)
                .is_some_and(|frame_end| frame_end <= self.end)
            && self
                .block_end(address)
                .is_some_and(|block_end| address + large <= block_end)
    }
}

impl Iterator for FramePlan<'_> {
    type Item = FrameRun;

    fn next(&mut self) -> Option<FrameRun> {
        if self.cursor >= self.end {
            return None;
        }
        let address = self.cursor;
        let block_end = self.block_end(address)?;
        let (size_bits, count) = if self.large_fits(address) {
            let large = 1_u64 << LARGE_FRAME_BITS;
            let mut count = 0_u32;
            while count < MAX_RETYPE_COUNT {
                let next = address + u64::from(count) * large;
                if !self.large_fits(next) || self.block_end(next) != Some(block_end) {
                    break;
                }
                count += 1;
            }
            (LARGE_FRAME_BITS, count)
        } else {
            let mut count = 0_u32;
            while count < MAX_RETYPE_COUNT {
                let next = address + u64::from(count) * PAGE_BYTES;
                if next >= self.end || next >= block_end || (count > 0 && self.large_fits(next)) {
                    break;
                }
                count += 1;
            }
            (PAGE_FRAME_BITS, count)
        };
        let run = FrameRun {
            address,
            size_bits,
            count,
        };
        self.cursor = run.end();
        Some(run)
    }
}

/// The sizes (`size_bits`) of the aligned chunks that carve a Untyped's
/// watermark forward from `from` to `to`, both multiples of 16 bytes with
/// `from <= to` (offsets from the block's base).
///
/// Retype allocates at the watermark, aligned to the object, so skipping
/// memory means carving it into Untypeds, each aligned to its own size: the
/// largest power of two that fits both the gap and the watermark's alignment.
#[derive(Clone, Debug)]
pub struct SkipChunks {
    from: u64,
    to: u64,
}

/// The chunks that move a watermark from `from` to `to`.
pub const fn skip_chunks(from: u64, to: u64) -> SkipChunks {
    SkipChunks { from, to }
}

impl Iterator for SkipChunks {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        if self.from >= self.to {
            return None;
        }
        let gap_bits = 63 - (self.to - self.from).leading_zeros();
        let alignment_bits = if self.from == 0 {
            63
        } else {
            self.from.trailing_zeros()
        };
        let bits = gap_bits.min(alignment_bits);
        self.from += 1_u64 << bits;
        u8::try_from(bits).ok()
    }
}
