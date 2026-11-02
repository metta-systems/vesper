//! How much memory a window's surface takes.
//!
//! A surface is made of whole 2 MiB Frames: its bytes, rounded up to a
//! multiple of 2 MiB. The rows are `width * 4` bytes apart. The slack at the
//! end lets a window grow a little without new memory.

/// Bytes of one Frame of a surface, and the size exponent the Frame is
/// created with.
pub const FRAME_BYTES: u64 = 2 << 20;
pub const FRAME_BITS: u8 = 21;

/// Most Frames of one surface.
pub const MAX_FRAMES: u32 = 8;

/// Bytes of one pixel.
pub const PIXEL_BYTES: u64 = 4;

/// Bytes from the start of one row to the start of the next.
#[must_use]
pub const fn pitch_bytes(width: u32) -> u64 {
    width as u64 * PIXEL_BYTES
}

/// How many Frames a surface of `width` by `height` pixels takes; `None` when
/// it has no pixels or does not fit [`MAX_FRAMES`].
#[must_use]
pub fn frames(width: u32, height: u32) -> Option<u32> {
    let bytes = pitch_bytes(width).saturating_mul(u64::from(height));
    if bytes == 0 {
        return None;
    }
    u32::try_from(bytes.div_ceil(FRAME_BYTES))
        .ok()
        .filter(|&count| count <= MAX_FRAMES)
}
