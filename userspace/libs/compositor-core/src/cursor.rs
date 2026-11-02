//! The pointer's sprite: the arrow of X-COM: UFO Defense (1994), in its four
//! yellow-green shades, brightest on its outline and darker towards its middle,
//! opaque and so already premultiplied.

use crate::windows::Rect;

/// Screen pixels per pixel of the art: X-COM ran at 320x200, so at 1024x768 the
/// arrow is drawn doubled.
const SCALE: u32 = 2;

/// Width and height of the art, and of the sprite on the screen.
const ART_WIDTH: u32 = 9;
const ART_HEIGHT: u32 = 13;
pub const WIDTH: u32 = ART_WIDTH * SCALE;
pub const HEIGHT: u32 = ART_HEIGHT * SCALE;

/// A digit is a shade of [`SHADES`] (`1` the first) and a space is
/// transparent. The hot spot is the top left pixel.
const SHAPE: [&[u8; ART_WIDTH as usize]; ART_HEIGHT as usize] = [
    b"1        ",
    b"11       ",
    b"121      ",
    b"1221     ",
    b"12321    ",
    b"123321   ",
    b"1234321  ",
    b"12344321 ",
    b"123444321",
    b"1234     ",
    b"123      ",
    b"12       ",
    b"1        ",
];

/// The shades, from the outline inwards: pale yellow to olive.
const SHADES: [u32; 4] = [0xfffc_fca4, 0xffdc_e88c, 0xffc0_d478, 0xffa3_c068];

/// The pixel of the sprite at (`x`, `y`) on the screen; transparent outside it.
#[must_use]
pub fn pixel(x: u32, y: u32) -> u32 {
    SHAPE
        .get((y / SCALE) as usize)
        .and_then(|row| row.get((x / SCALE) as usize))
        .and_then(|&mark| mark.checked_sub(b'1'))
        .and_then(|shade| SHADES.get(usize::from(shade)))
        .copied()
        .unwrap_or(0)
}

/// The screen rectangle the sprite covers with its hot spot at (`x`, `y`).
#[must_use]
pub const fn rect_at(x: i32, y: i32) -> Rect {
    Rect {
        x,
        y,
        width: WIDTH,
        height: HEIGHT,
    }
}
