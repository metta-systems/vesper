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
