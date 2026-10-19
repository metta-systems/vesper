//! The pointer's sprite: an arrow, white with a black outline, premultiplied.

use crate::windows::Rect;

/// Width and height of the sprite in pixels.
pub const WIDTH: u32 = 12;
pub const HEIGHT: u32 = 19;

/// `#` is black, `.` is white and a space is transparent. The hot spot is the
/// top left pixel.
const SHAPE: [&[u8; WIDTH as usize]; HEIGHT as usize] = [
    b"#           ",
    b"##          ",
    b"#.#         ",
    b"#..#        ",
    b"#...#       ",
    b"#....#      ",
    b"#.....#     ",
    b"#......#    ",
    b"#.......#   ",
    b"#........#  ",
    b"#.....#####.",
    b"#..#..#     ",
    b"#.# #..#    ",
    b"##  #..#    ",
    b"#    #..#   ",
    b"     #..#   ",
    b"      #..#  ",
    b"      #..#  ",
    b"       ##   ",
];

const BLACK: u32 = 0xff00_0000;
const WHITE: u32 = 0xffff_ffff;

/// The pixel of the sprite at (`x`, `y`); transparent outside it.
#[must_use]
pub fn pixel(x: u32, y: u32) -> u32 {
    SHAPE
        .get(y as usize)
        .and_then(|row| row.get(x as usize))
        .map_or(0, |&mark| match mark {
            b'#' => BLACK,
            b'.' => WHITE,
            _ => 0,
        })
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
