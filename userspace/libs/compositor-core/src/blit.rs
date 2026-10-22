//! Drawing the screen from layers: clipped, premultiplied-alpha blits onto a
//! canvas, then a copy of the finished rectangles to the display.
//!
//! A pixel is a 32-bit word with alpha in the highest byte and three color
//! channels below it, already multiplied by the alpha. The channel order does
//! not matter to the arithmetic, so the same code serves RGB and BGR displays.

use crate::{cursor, windows::Rect};

/// Something pixels can be read from, by index (row times pitch plus column).
pub trait PixelRead {
    fn pixel(&self, index: usize) -> u32;
}

impl PixelRead for [u32] {
    fn pixel(&self, index: usize) -> u32 {
        self.get(index).copied().unwrap_or(0)
    }
}

/// A slice of pixels as a layer's source.
pub struct Pixels<'pixels>(pub &'pixels [u32]);

impl PixelRead for Pixels<'_> {
    fn pixel(&self, index: usize) -> u32 {
        self.0.pixel(index)
    }
}

/// Something pixels can be written to, by index.
pub trait PixelWrite {
    fn set_pixel(&mut self, index: usize, value: u32);
}

impl PixelWrite for [u32] {
    fn set_pixel(&mut self, index: usize, value: u32) {
        if let Some(slot) = self.get_mut(index) {
            *slot = value;
        }
    }
}

/// Alpha of a pixel.
const fn alpha(pixel: u32) -> u32 {
    pixel >> 24
}

/// Put `source` over `destination`; both premultiplied, the destination
/// opaque, so the result is opaque.
#[must_use]
pub fn blend(source: u32, destination: u32) -> u32 {
    let source_alpha = alpha(source);
    match source_alpha {
        255 => source,
        0 => destination,
        _ => {
            let inverse = 255 - source_alpha;
            let mut result = 0xff00_0000;
            for shift in [0, 8, 16] {
                let over = (source >> shift) & 0xff;
                let under = (destination >> shift) & 0xff;
                let channel = (over + (under * inverse + 127) / 255).min(255);
                result |= channel << shift;
            }
            result
        }
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

    /// The pixels of row `row` from column `column`, `count` of them, as indices.
    fn start(&self, column: u32, row: u32) -> usize {
        row as usize * self.pitch + column as usize
    }

    /// Fill `area`, which lies inside the canvas, with `pixel`.
    fn fill(&mut self, area: &Rect, pixel: u32) {
        let (column, row) = (area.x.unsigned_abs(), area.y.unsigned_abs());
        for line in 0..area.height {
            let start = self.start(column, row + line);
            let end = start + area.width as usize;
            if let Some(slice) = self.pixels.get_mut(start..end) {
                slice.fill(pixel);
            }
        }
    }
}

/// A window's pixels to draw: where its top left corner is on the screen,
/// how much of it shows, and where the pixels are.
pub struct Layer<'pixels> {
    pub rect: Rect,
    pub pixels: &'pixels dyn PixelRead,
    /// Pixels from one row of the surface to the next.
    pub pitch: usize,
}

/// Draw the part of the screen inside `damage`: `background`, then each of
/// `layers` bottom to top, then the cursor sprite with its hot spot at
/// `cursor`, if given. Everything is clipped to the canvas and to `damage`.
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
    canvas.fill(&clip, background);
    for layer in layers {
        let Some(area) = clip.intersect(&layer.rect) else {
            continue;
        };
        let origin_x = i64::from(layer.rect.x);
        let origin_y = i64::from(layer.rect.y);
        for line in 0..area.height {
            let row = area.y.unsigned_abs() + line;
            let surface_row = i64::from(row) - origin_y;
            for offset in 0..area.width {
                let column = area.x.unsigned_abs() + offset;
                let surface_column = i64::from(column) - origin_x;
                let source_index = usize::try_from(surface_row)
                    .unwrap_or(0)
                    .wrapping_mul(layer.pitch)
                    .wrapping_add(usize::try_from(surface_column).unwrap_or(0));
                let source = layer.pixels.pixel(source_index);
                let index = canvas.start(column, row);
                let blended = blend(source, canvas.pixels.pixel(index));
                canvas.pixels.set_pixel(index, blended);
            }
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
    for line in 0..area.height {
        let row = area.y.unsigned_abs() + line;
        for offset in 0..area.width {
            let column = area.x.unsigned_abs() + offset;
            let source = cursor::pixel(
                u32::try_from(i64::from(column) - i64::from(sprite.x)).unwrap_or(0),
                u32::try_from(i64::from(row) - i64::from(sprite.y)).unwrap_or(0),
            );
            if alpha(source) == 0 {
                continue;
            }
            let index = canvas.start(column, row);
            let blended = blend(source, canvas.pixels.pixel(index));
            canvas.pixels.set_pixel(index, blended);
        }
    }
}

/// Copy `area` of `canvas` to `display`, a surface of `display_pitch` pixels a
/// row with the same origin. `area` is clipped to the canvas.
pub fn present<D: PixelWrite + ?Sized>(
    canvas: &Canvas<'_>,
    display: &mut D,
    display_pitch: usize,
    area: &Rect,
) {
    let Some(area) = area.intersect(&canvas.bounds()) else {
        return;
    };
    let column = area.x.unsigned_abs();
    for line in 0..area.height {
        let row = area.y.unsigned_abs() + line;
        for offset in 0..area.width {
            let source = canvas.start(column + offset, row);
            let target = row as usize * display_pitch + (column + offset) as usize;
            display.set_pixel(target, canvas.pixels.pixel(source));
        }
    }
}
