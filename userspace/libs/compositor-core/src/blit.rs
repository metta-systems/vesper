//! Drawing the screen from layers: clipped, premultiplied-alpha blits onto a
//! canvas, then a copy of the finished rectangles to an output, converted to
//! its pixel format.
//!
//! A pixel is a 32-bit word with alpha in the highest byte and three color
//! channels below it, already multiplied by the alpha. The channel order does
//! not matter to the blending arithmetic, so the same code serves RGB and BGR
//! canvases; it matters only when [`present`] converts to an output's format.
//!
//! The work is done a row at a time, on slices: a row of a layer is blended
//! onto a row of the canvas, and a row of the canvas is converted into a row
//! of the output, in loops without branches on the pixel values, so that the
//! compiler can vectorize them.

use crate::{cursor, windows::Rect};

/// How pixels sit in an output's memory (`display_v1`'s `format`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// 32-bit, blue in the lowest byte.
    Xrgb8888,
    /// 32-bit, red in the lowest byte.
    Xbgr8888,
    /// 16-bit 5-6-5 with red in the top bits, the high byte first.
    Rgb565Be,
    /// 16-bit 5-6-5 with red in the top bits, the low byte first.
    Rgb565,
}

impl Format {
    /// Bytes of one pixel.
    #[must_use]
    pub const fn pixel_bytes(self) -> usize {
        match self {
            Self::Xrgb8888 | Self::Xbgr8888 => 4,
            Self::Rgb565Be | Self::Rgb565 => 2,
        }
    }

    /// Whether pixels are 16-bit 5-6-5: such an output is drawn from a
    /// 32-bit canvas by conversion.
    #[must_use]
    pub const fn is_rgb565(self) -> bool {
        matches!(self, Self::Rgb565Be | Self::Rgb565)
    }
}

/// Swap the lowest and the third byte of a 32-bit pixel: `xrgb8888` to
/// `xbgr8888` and back.
#[must_use]
pub const fn swap_red_blue(pixel: u32) -> u32 {
    (pixel & 0xff00_ff00) | ((pixel >> 16) & 0xff) | ((pixel & 0xff) << 16)
}

/// The 5-6-5 value of `pixel`, a pixel of a canvas in `canvas` format: each
/// channel keeps its top bits, red 5, green 6 and blue 5.
#[must_use]
pub const fn rgb565(pixel: u32, canvas: Format) -> u16 {
    let xrgb = match canvas {
        Format::Xbgr8888 => swap_red_blue(pixel),
        Format::Xrgb8888 | Format::Rgb565Be | Format::Rgb565 => pixel,
    };
    let red = (xrgb >> 19) & 0x1f;
    let green = (xrgb >> 10) & 0x3f;
    let blue = (xrgb >> 3) & 0x1f;
    #[allow(clippy::cast_possible_truncation, reason = "5 + 6 + 5 bits fit 16")]
    let packed = ((red << 11) | (green << 5) | blue) as u16;
    packed
}

/// Alpha of a pixel.
const fn alpha(pixel: u32) -> u32 {
    pixel >> 24
}

/// Put `source` over `destination`; both premultiplied, the destination
/// opaque, so the result is opaque. A fully transparent source leaves the
/// destination as it was.
///
/// Each channel is `over + under * (255 - alpha) / 255`, rounded, which for an
/// opaque source is the source. There is no early return on the alpha, only a
/// select at the end, so that a loop of blends vectorizes.
#[inline]
#[must_use]
pub const fn blend(source: u32, destination: u32) -> u32 {
    let source_alpha = alpha(source);
    let inverse = 255 - source_alpha;
    let mut result = 0xff00_0000;
    let mut shift = 0;
    while shift < 24 {
        let over = (source >> shift) & 0xff;
        let under = (destination >> shift) & 0xff;
        let channel = over + (under * inverse + 127) / 255;
        let channel = if channel > 255 { 255 } else { channel };
        result |= channel << shift;
        shift += 8;
    }
    if source_alpha == 0 {
        destination
    } else {
        result
    }
}

/// Blend each pixel of `source` over the pixel of `destination` at the same
/// position; the shorter slice decides how many.
pub fn blend_row(destination: &mut [u32], source: &[u32]) {
    for (under, &over) in destination.iter_mut().zip(source) {
        *under = blend(over, *under);
    }
}

/// The pixels of a screen being drawn: `width` by `height` of them, `pitch`
/// pixels apart from one row to the next.
pub struct Canvas<'pixels> {
    pub pixels: &'pixels mut [u32],
    pub pitch: usize,
    pub width: u32,
    pub height: u32,
}

impl Canvas<'_> {
    /// The canvas as a rectangle at the origin.
    #[must_use]
    pub const fn bounds(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }

    /// The index of the pixel at `column` of `row`.
    const fn start(&self, column: u32, row: u32) -> usize {
        row as usize * self.pitch + column as usize
    }

    /// The pixels of row `line` of `area`, which lies inside the canvas.
    fn row_mut(&mut self, area: &Rect, line: u32) -> Option<&mut [u32]> {
        let start = self.start(area.x.unsigned_abs(), area.y.unsigned_abs() + line);
        self.pixels.get_mut(start..start + area.width as usize)
    }

    /// The pixels of row `line` of `area`, which lies inside the canvas.
    fn row(&self, area: &Rect, line: u32) -> Option<&[u32]> {
        let start = self.start(area.x.unsigned_abs(), area.y.unsigned_abs() + line);
        self.pixels.get(start..start + area.width as usize)
    }
}

/// A window's pixels to draw: where its top left corner is on the canvas, and
/// its rows, `pitch` pixels apart.
pub struct Layer<'pixels> {
    pub rect: Rect,
    pub pixels: &'pixels [u32],
    /// Pixels from one row of the surface to the next.
    pub pitch: usize,
}

impl Layer<'_> {
    /// The layer's pixels under row `line` of `area`, which lies inside the
    /// layer.
    fn row(&self, area: &Rect, line: u32) -> Option<&[u32]> {
        let surface_row = i64::from(area.y) + i64::from(line) - i64::from(self.rect.y);
        let surface_column = i64::from(area.x) - i64::from(self.rect.x);
        let start = usize::try_from(surface_row)
            .ok()?
            .checked_mul(self.pitch)?
            .checked_add(usize::try_from(surface_column).ok()?)?;
        self.pixels
            .get(start..start.checked_add(area.width as usize)?)
    }
}

/// Draw the part of the canvas inside `damage`: `background`, then each of
/// `layers` bottom to top, then the cursor sprite with its hot spot at
/// `cursor`, if given. Everything is clipped to the canvas and to `damage`.
/// A row that the layer's pixels do not cover is left out.
pub fn compose(
    canvas: &mut Canvas<'_>,
    damage: &Rect,
    background: u32,
    layers: &[Layer<'_>],
    cursor: Option<(i32, i32)>,
) {
    let Some(clip) = damage.intersect(&canvas.bounds()) else {
        return;
    };
    for line in 0..clip.height {
        if let Some(row) = canvas.row_mut(&clip, line) {
            row.fill(background);
        }
    }
    for layer in layers {
        let Some(area) = clip.intersect(&layer.rect) else {
            continue;
        };
        for line in 0..area.height {
            let (Some(source), Some(destination)) =
                (layer.row(&area, line), canvas.row_mut(&area, line))
            else {
                continue;
            };
            blend_row(destination, source);
        }
    }
    if let Some((x, y)) = cursor {
        draw_cursor(canvas, &clip, x, y);
    }
}

/// Draw the cursor sprite with its hot spot at (`x`, `y`), inside `clip`.
fn draw_cursor(canvas: &mut Canvas<'_>, clip: &Rect, x: i32, y: i32) {
    let sprite = cursor::rect_at(x, y);
    let Some(area) = clip.intersect(&sprite) else {
        return;
    };
    let first_column = u32::try_from(i64::from(area.x) - i64::from(sprite.x)).unwrap_or(0);
    let first_row = u32::try_from(i64::from(area.y) - i64::from(sprite.y)).unwrap_or(0);
    for line in 0..area.height {
        let Some(row) = canvas.row_mut(&area, line) else {
            continue;
        };
        for (offset, under) in (0_u32..).zip(row.iter_mut()) {
            *under = blend(
                cursor::pixel(first_column + offset, first_row + line),
                *under,
            );
        }
    }
}

/// Copy `area` of `canvas`, whose pixels are in `canvas_format` (a 32-bit
/// format), to `output`, converting them to `output_format`. The output has
/// the canvas's origin and `output_pitch` bytes from one row to the next.
/// `area` is clipped to the canvas; rows past the end of `output` are left
/// out.
pub fn present(
    canvas: &Canvas<'_>,
    canvas_format: Format,
    output: &mut [u8],
    output_pitch: usize,
    output_format: Format,
    area: &Rect,
) {
    let Some(area) = area.intersect(&canvas.bounds()) else {
        return;
    };
    let pixel_bytes = output_format.pixel_bytes();
    let first = area.x.unsigned_abs() as usize * pixel_bytes;
    let length = area.width as usize * pixel_bytes;
    for line in 0..area.height {
        let row = (area.y.unsigned_abs() + line) as usize;
        let start = row * output_pitch + first;
        let (Some(source), Some(target)) = (
            canvas.row(&area, line),
            output.get_mut(start..start + length),
        ) else {
            continue;
        };
        convert_row(source, canvas_format, target, output_format);
    }
}

/// [`present`] for an output the compositor turns while drawing: the canvas
/// is the viewport, turned `turns` clockwise quarter turns from the output's
/// own memory, which is `native` pixels wide and high
/// ([`native_point`](crate::layout::native_point)). A canvas whose size is not
/// `native` turned is left alone. Each pixel lands on a different row of the
/// output for a quarter turn, so this goes pixel by pixel; it serves outputs
/// that cannot turn themselves.
#[allow(
    clippy::too_many_arguments,
    reason = "present's six, and the turn and the unturned size"
)]
pub fn present_turned(
    canvas: &Canvas<'_>,
    canvas_format: Format,
    output: &mut [u8],
    output_pitch: usize,
    output_format: Format,
    area: &Rect,
    turns: u8,
    native: (u32, u32),
) {
    if turns % 4 == 0 {
        present(
            canvas,
            canvas_format,
            output,
            output_pitch,
            output_format,
            area,
        );
        return;
    }
    if crate::layout::turned_size(native, turns) != (canvas.width, canvas.height) {
        return;
    }
    let Some(area) = area.intersect(&canvas.bounds()) else {
        return;
    };
    let pixel_bytes = output_format.pixel_bytes();
    for line in 0..area.height {
        let Some(source) = canvas.row(&area, line) else {
            continue;
        };
        let y = area.y.unsigned_abs() + line;
        for (offset, &pixel) in (0_u32..).zip(source) {
            let x = area.x.unsigned_abs() + offset;
            let (native_x, native_y) = crate::layout::native_point((x, y), turns, native);
            let start = native_y as usize * output_pitch + native_x as usize * pixel_bytes;
            if let Some(target) = output.get_mut(start..start + pixel_bytes) {
                convert_row(&[pixel], canvas_format, target, output_format);
            }
        }
    }
}

/// Convert one row of canvas pixels in `canvas_format` into `target`, as
/// pixels in `output_format`. There is one loop per pair of formats, so the
/// inner loop does not branch.
pub fn convert_row(
    source: &[u32],
    canvas_format: Format,
    target: &mut [u8],
    output_format: Format,
) {
    match (canvas_format, output_format) {
        (Format::Xrgb8888, Format::Xbgr8888) | (Format::Xbgr8888, Format::Xrgb8888) => {
            for (bytes, &pixel) in target.as_chunks_mut::<4>().0.iter_mut().zip(source) {
                *bytes = swap_red_blue(pixel).to_le_bytes();
            }
        }
        (_, Format::Rgb565Be) => {
            for (bytes, &pixel) in target.as_chunks_mut::<2>().0.iter_mut().zip(source) {
                *bytes = rgb565(pixel, canvas_format).to_be_bytes();
            }
        }
        (_, Format::Rgb565) => {
            for (bytes, &pixel) in target.as_chunks_mut::<2>().0.iter_mut().zip(source) {
                *bytes = rgb565(pixel, canvas_format).to_le_bytes();
            }
        }
        _ => {
            for (bytes, &pixel) in target.as_chunks_mut::<4>().0.iter_mut().zip(source) {
                *bytes = pixel.to_le_bytes();
            }
        }
    }
}
