#![no_std]

//! The screen geometry of the ILI9341 panel driver, kept apart from the
//! component so host tests reach it: the screen a `display_v1` client writes
//! in its data area (big-endian RGB565, rows of `width * 2` bytes), which
//! rectangles of it `flush` accepts, the byte spans a rectangle's rows take,
//! and the size the screen has after a turn.
//!
//! The controller turns its scan direction itself (MADCTL, ILI9341 datasheet
//! V1.11 section 8.2.29, Memory Access Control, 36h), so all four turns are
//! the panel's own and only swap width and height.

/// Bytes of one pixel: RGB565.
pub const BYTES_PER_PIXEL: u32 = 2;

/// The turns the panel does itself, as `display_v1`'s set of rotations (bit
/// `n`: `n` quarter turns clockwise): all four.
pub const ROTATIONS: u32 = 0b1111;

/// The screen as the panel is turned now: its size in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Screen {
    /// The bytes of one row.
    #[must_use]
    pub const fn pitch(self) -> u32 {
        self.width * BYTES_PER_PIXEL
    }

    /// The bytes of the whole screen.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.pitch() as u64 * self.height as u64
    }

    /// The screen of a panel whose unturned size is `self`, after
    /// `quarter_turns` quarter turns: a quarter or three-quarter turn swaps
    /// width and height.
    #[must_use]
    pub const fn turned(self, quarter_turns: u32) -> Self {
        if quarter_turns % 2 == 1 {
            Self {
                width: self.height,
                height: self.width,
            }
        } else {
            self
        }
    }
}

/// A rectangle of the screen, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rectangle {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rectangle {
    /// Whether the rectangle is not empty and lies inside `screen`.
    #[must_use]
    pub fn within(self, screen: Screen) -> bool {
        let fits = |start: u32, length: u32, limit: u32| {
            length != 0 && start.checked_add(length).is_some_and(|end| end <= limit)
        };
        fits(self.x, self.width, screen.width) && fits(self.y, self.height, screen.height)
    }

    /// The last column and row the rectangle covers (inclusive, as the
    /// controller's column and page address commands take them). Only for a
    /// rectangle [`within`](Self::within) a screen.
    #[must_use]
    pub const fn last(self) -> (u32, u32) {
        (self.x + self.width - 1, self.y + self.height - 1)
    }

    /// The byte spans of the rectangle's rows in `screen`, top to bottom: the
    /// offset from the screen's first byte and the length. Only for a
    /// rectangle [`within`](Self::within) the screen.
    pub fn rows(self, screen: Screen) -> impl Iterator<Item = (u64, u64)> {
        let pitch = u64::from(screen.pitch());
        let start = u64::from(self.x) * u64::from(BYTES_PER_PIXEL);
        let length = u64::from(self.width) * u64::from(BYTES_PER_PIXEL);
        (self.y..self.y + self.height).map(move |row| (u64::from(row) * pitch + start, length))
    }
}

/// The pixel the two bytes of the screen hold: RGB565, the high byte first.
#[must_use]
pub const fn rgb565_be(bytes: [u8; 2]) -> u16 {
    u16::from_be_bytes(bytes)
}
