#![no_std]

//! Drawing on a framebuffer: [`Color`]s, filled rectangles and text from a
//! bitmap [`Font`], onto any [`Surface`] that can store a pixel.
//!
//! [`Lava`] animates metaballs, "lava lamp" blobs, with integer arithmetic only
//! (components run without floating point).
//!
//! The library knows nothing of where the pixels live: the shell's surface
//! writes them through a mapping of the framebuffer's Frames, the host tests'
//! into a vector.

/// The order of the color components in a framebuffer pixel, as the firmware
/// reports it (`fb_v1.pixel_order`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelOrder {
    /// Blue is the lowest byte of the pixel word
    Bgr,
    /// Red is the lowest byte of the pixel word
    Rgb,
}

/// A color, as its red, green and blue intensities; [`Color::encode`] makes
/// the pixel word of a framebuffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub const fn black() -> Self {
        Self::rgb(0, 0, 0)
    }

    pub const fn white() -> Self {
        Self::rgb(255, 255, 255)
    }

    pub const fn red() -> Self {
        Self::rgb(255, 0, 0)
    }

    pub const fn green() -> Self {
        Self::rgb(0, 255, 0)
    }

    pub const fn blue() -> Self {
        Self::rgb(0, 0, 255)
    }

    /// The pixel word of the color in a framebuffer of the given `order`;
    /// the highest byte (alpha) is zero.
    pub const fn encode(self, order: PixelOrder) -> u32 {
        let red = self.red as u32;
        let green = self.green as u32;
        let blue = self.blue as u32;
        match order {
            PixelOrder::Rgb => (blue << 16) | (green << 8) | red,
            PixelOrder::Bgr => (red << 16) | (green << 8) | blue,
        }
    }
}

/// A bitmap font: 8x8 glyphs for the characters from space up, 8 bytes each,
/// top row first, bit 0 the leftmost pixel (the layout of the boot image's
/// font resource).
#[derive(Clone, Copy, Debug)]
pub struct Font<'data> {
    data: &'data [u8],
}

impl<'data> Font<'data> {
    /// Bytes of one glyph.
    pub const GLYPH_BYTES: usize = 8;
    /// Width and height of a glyph, in pixels.
    pub const CELL: u32 = 8;
    /// The first character the font has.
    pub const FIRST: u8 = b' ';

    /// The font held in `data`, if it is a whole number of glyphs.
    pub const fn new(data: &'data [u8]) -> Option<Self> {
        if data.is_empty() || !data.len().is_multiple_of(Self::GLYPH_BYTES) {
            None
        } else {
            Some(Self { data })
        }
    }

    /// How many glyphs the font has.
    pub const fn glyph_count(&self) -> usize {
        self.data.len() / Self::GLYPH_BYTES
    }

    /// The rows of the glyph for `character`, if the font has it.
    pub fn glyph(&self, character: u8) -> Option<&'data [u8; 8]> {
        let index = usize::from(character.checked_sub(Self::FIRST)?);
        self.data
            .get(index * Self::GLYPH_BYTES..)?
            .first_chunk::<8>()
    }
}

/// Somewhere pixels can be stored.
pub trait Surface {
    /// Width in pixels.
    fn width(&self) -> u32;
    /// Height in pixels.
    fn height(&self) -> u32;
    /// Store `pixel` at (`x`, `y`), which is inside the surface.
    fn put(&mut self, x: u32, y: u32, pixel: u32);
}

/// Text could not be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawError {
    /// The font has no glyph for the byte; the characters before it are drawn
    CharacterNotInFont(u8),
}

/// Draws on a [`Surface`] in a pixel order.
pub struct Painter<'surface, S: Surface> {
    surface: &'surface mut S,
    order: PixelOrder,
}

impl<'surface, S: Surface> Painter<'surface, S> {
    pub fn new(surface: &'surface mut S, order: PixelOrder) -> Self {
        Self { surface, order }
    }

    pub fn width(&self) -> u32 {
        self.surface.width()
    }

    pub fn height(&self) -> u32 {
        self.surface.height()
    }

    /// Fill the rectangle of `width` by `height` pixels at (`x`, `y`) with
    /// `color`; the part outside the surface is left out.
    pub fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let pixel = color.encode(self.order);
        let right = x.saturating_add(width).min(self.surface.width());
        let bottom = y.saturating_add(height).min(self.surface.height());
        for row in y..bottom {
            for column in x..right {
                self.surface.put(column, row, pixel);
            }
        }
    }

    /// Fill the whole surface with `color`.
    pub fn clear(&mut self, color: Color) {
        self.rect(0, 0, self.width(), self.height(), color);
    }

    /// Draw `text` with its top left corner at (`x`, `y`), one 8x8 cell per
    /// byte, the set pixels of each glyph in `color`.
    pub fn text(
        &mut self,
        x: u32,
        y: u32,
        text: &str,
        font: &Font<'_>,
        color: Color,
    ) -> Result<(), DrawError> {
        let pixel = color.encode(self.order);
        let mut cell_x = x;
        for &character in text.as_bytes() {
            let glyph = font
                .glyph(character)
                .ok_or(DrawError::CharacterNotInFont(character))?;
            for (row, bits) in (0..Font::CELL).zip(glyph) {
                for column in 0..Font::CELL {
                    let (pixel_x, pixel_y) = (cell_x + column, y + row);
                    if bits >> column & 1 == 1
                        && pixel_x < self.surface.width()
                        && pixel_y < self.surface.height()
                    {
                        self.surface.put(pixel_x, pixel_y, pixel);
                    }
                }
            }
            cell_x += Font::CELL;
        }
        Ok(())
    }
}

/// Blobs a [`Lava`] lamp holds.
pub const BLOBS: usize = 5;

/// Fixed-point fraction bits of blob positions and speeds.
const FRACTION_BITS: u32 = 8;

#[derive(Clone, Copy)]
struct Blob {
    /// Center, in 1/256 pixels
    x: i32,
    y: i32,
    /// Speed, in 1/256 pixels per step
    dx: i32,
    dy: i32,
    /// Radius in pixels
    radius: u32,
}

/// Metaballs drifting inside a rectangle of cells, drawn as a glowing field.
///
/// The area is `columns` by `rows` cells of `cell` pixels; each cell is one
/// field sample, so a coarse `cell` makes a frame cheap. The field at a cell
/// sums `radius^2 / distance^2` of every blob, in 1/16 units, and a color
/// ramp turns it into pixels: dark red where it is weak, through orange to a
/// bright yellow core where blobs merge.
pub struct Lava {
    columns: u32,
    rows: u32,
    cell: u32,
    blobs: [Blob; BLOBS],
}

impl Lava {
    /// A lamp of `columns` by `rows` cells of `cell` pixels, the blobs spread
    /// out from `seed`.
    pub fn new(columns: u32, rows: u32, cell: u32, seed: u32) -> Self {
        let mut state = seed | 1;
        let mut next = |bound: u32| {
            // xorshift32
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state % bound.max(1)
        };
        let width = columns * cell;
        let height = rows * cell;
        let mut blobs = [Blob {
            x: 0,
            y: 0,
            dx: 0,
            dy: 0,
            radius: 0,
        }; BLOBS];
        for blob in &mut blobs {
            let radius = height / 10 + next(height / 10);
            blob.radius = radius.max(1);
            blob.x = i32::try_from(
                (radius + next(width.saturating_sub(2 * radius).max(1))) << FRACTION_BITS,
            )
            .unwrap_or(0);
            blob.y = i32::try_from(
                (radius + next(height.saturating_sub(2 * radius).max(1))) << FRACTION_BITS,
            )
            .unwrap_or(0);
            // Between 0.5 and 2.5 pixels per step, either direction.
            let signed_step = |value: u32| {
                let magnitude = i32::try_from(128 + value % 512).unwrap_or(128);
                if value & 0x1000 == 0 {
                    magnitude
                } else {
                    -magnitude
                }
            };
            blob.dx = signed_step(next(0x2000));
            blob.dy = signed_step(next(0x2000));
        }
        Self {
            columns,
            rows,
            cell,
            blobs,
        }
    }

    /// Move every blob one step, bouncing off the edges.
    pub fn step(&mut self) {
        let width = i32::try_from((self.columns * self.cell) << FRACTION_BITS).unwrap_or(i32::MAX);
        let height = i32::try_from((self.rows * self.cell) << FRACTION_BITS).unwrap_or(i32::MAX);
        for blob in &mut self.blobs {
            let margin = i32::try_from(blob.radius << FRACTION_BITS).unwrap_or(0);
            blob.x += blob.dx;
            blob.y += blob.dy;
            if blob.x < margin || blob.x > width - margin {
                blob.dx = -blob.dx;
                blob.x = blob.x.clamp(margin, (width - margin).max(margin));
            }
            if blob.y < margin || blob.y > height - margin {
                blob.dy = -blob.dy;
                blob.y = blob.y.clamp(margin, (height - margin).max(margin));
            }
        }
    }

    /// The field at the middle of cell (`column`, `row`), in 1/16 units
    /// (16 = one blob's edge), saturating at 255.
    pub fn field(&self, column: u32, row: u32) -> u32 {
        let half = self.cell / 2;
        let x = i64::from(column * self.cell + half);
        let y = i64::from(row * self.cell + half);
        let mut sum: u64 = 0;
        for blob in &self.blobs {
            let dx = x - i64::from(blob.x >> FRACTION_BITS);
            let dy = y - i64::from(blob.y >> FRACTION_BITS);
            let distance_squared = (dx * dx + dy * dy).max(1).unsigned_abs();
            let radius_squared = u64::from(blob.radius) * u64::from(blob.radius);
            sum += radius_squared * 16 / distance_squared;
        }
        u32::try_from(sum.min(255)).unwrap_or(255)
    }

    /// The color of a field `value`: nothing below the edge of influence
    /// (8), then a ramp from dark red through orange to bright yellow.
    pub fn ramp(value: u32) -> Color {
        if value < 8 {
            return Color::black();
        }
        let level = ((value - 8) * 255 / 247).min(255);
        let channel = |value: u32| u8::try_from(value.min(255)).unwrap_or(255);
        Color::rgb(
            channel(level * 2).max(48),
            channel(level.saturating_sub(127) * 2),
            channel(level.saturating_sub(224) * 8),
        )
    }

    /// Draw the lamp with its top left corner at (`x`, `y`): one filled cell
    /// per field sample.
    pub fn draw<S: Surface>(&self, painter: &mut Painter<'_, S>, x: u32, y: u32) {
        for row in 0..self.rows {
            for column in 0..self.columns {
                painter.rect(
                    x + column * self.cell,
                    y + row * self.cell,
                    self.cell,
                    self.cell,
                    Self::ramp(self.field(column, row)),
                );
            }
        }
    }
}
