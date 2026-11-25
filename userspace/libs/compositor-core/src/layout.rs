//! The arrangement of the outputs on the desktop.
//!
//! The desktop is one coordinate space, and each output is a viewport into it:
//! the rectangle of the desktop that the output shows. Viewports may touch, be
//! far apart, or form an L. The pointer roams their union: it crosses an edge
//! where two outputs touch and stops where no output continues.

use crate::windows::Rect;

/// Outputs the compositor drives at once.
pub const MAX_OUTPUTS: usize = 2;

/// The size of an output of `width` by `height` after `turns` clockwise
/// quarter turns: an odd number swaps them.
#[must_use]
pub const fn turned_size((width, height): (u32, u32), turns: u8) -> (u32, u32) {
    if turns % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    }
}

/// Where the pixel (`x`, `y`) of a viewport turned by `turns` clockwise
/// quarter turns lies in the output's own memory, `native` pixels wide and
/// high as the output stands unturned. A clockwise turn puts the viewport's
/// top edge on the output's right edge, so its top left pixel at the top right.
#[must_use]
pub const fn native_point(
    (x, y): (u32, u32),
    turns: u8,
    (native_width, native_height): (u32, u32),
) -> (u32, u32) {
    match turns % 4 {
        1 => (native_width - 1 - y, x),
        2 => (native_width - 1 - x, native_height - 1 - y),
        3 => (y, native_height - 1 - x),
        _ => (x, y),
    }
}

/// The rectangle of the output's own memory that `rect`, a rectangle of a
/// viewport turned by `turns` (see [`native_point`]), covers. `rect` lies
/// inside the viewport.
#[must_use]
pub const fn native_rect(rect: Rect, turns: u8, (native_width, native_height): (u32, u32)) -> Rect {
    let (x, y) = (rect.x.unsigned_abs(), rect.y.unsigned_abs());
    let (left, top, width, height) = match turns % 4 {
        1 => (native_width - y - rect.height, x, rect.height, rect.width),
        2 => (
            native_width - x - rect.width,
            native_height - y - rect.height,
            rect.width,
            rect.height,
        ),
        3 => (y, native_height - x - rect.width, rect.height, rect.width),
        _ => (x, y, rect.width, rect.height),
    };
    #[allow(
        clippy::cast_possible_wrap,
        reason = "inside an output, far below i32::MAX"
    )]
    Rect {
        x: left as i32,
        y: top as i32,
        width,
        height,
    }
}

/// The viewport of each output, by the output's index; `None` where there is
/// no output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    viewports: [Option<Rect>; MAX_OUTPUTS],
}

impl Layout {
    /// No outputs.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            viewports: [None; MAX_OUTPUTS],
        }
    }

    /// One output of `width` by `height` pixels at the origin.
    #[must_use]
    pub const fn single(width: u32, height: u32) -> Self {
        let mut layout = Self::new();
        layout.viewports[0] = Some(Rect {
            x: 0,
            y: 0,
            width,
            height,
        });
        layout
    }

    /// The default arrangement: the outputs of `sizes` (by index; `None` where
    /// there is none) left to right in index order, their tops at y = 0, the
    /// first one's top left corner the origin. Sizes past [`MAX_OUTPUTS`] are
    /// left out.
    #[must_use]
    pub fn row(sizes: &[Option<(u32, u32)>]) -> Self {
        let mut layout = Self::new();
        let mut x = 0_i32;
        for (viewport, size) in layout.viewports.iter_mut().zip(sizes) {
            let Some((width, height)) = *size else {
                continue;
            };
            *viewport = Some(Rect {
                x,
                y: 0,
                width,
                height,
            });
            x = x.saturating_add_unsigned(width);
        }
        layout
    }

    /// The viewport of output `index`.
    #[must_use]
    pub fn viewport(&self, index: usize) -> Option<Rect> {
        self.viewports.get(index).copied().flatten()
    }

    /// Set output `index`'s viewport, or take it away with `None`.
    pub fn set(&mut self, index: usize, viewport: Option<Rect>) {
        if let Some(slot) = self.viewports.get_mut(index) {
            *slot = viewport;
        }
    }

    /// Each output's index and viewport.
    pub fn viewports(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        self.viewports
            .iter()
            .enumerate()
            .filter_map(|(index, viewport)| viewport.map(|viewport| (index, viewport)))
    }

    /// Whether there is no output.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.viewports().next().is_none()
    }

    /// The output showing the pixel (`x`, `y`): the first whose viewport holds
    /// it.
    #[must_use]
    pub fn output_at(&self, x: i32, y: i32) -> Option<usize> {
        self.viewports()
            .find(|(_index, viewport)| viewport.contains(x, y))
            .map(|(index, _viewport)| index)
    }

    /// Whether `viewport`, as output `index`'s, would overlap another output's
    /// viewport. Touching edges do not overlap.
    #[must_use]
    pub fn overlaps(&self, index: usize, viewport: &Rect) -> bool {
        self.viewports()
            .any(|(other, rect)| other != index && rect.intersect(viewport).is_some())
    }

    /// The output that shows most of the desktop rectangle `rect`; the first
    /// of equals. None when no output shows any of it.
    #[must_use]
    pub fn output_of(&self, rect: &Rect) -> Option<usize> {
        let mut best: Option<(usize, u64)> = None;
        for (index, viewport) in self.viewports() {
            let Some(shown) = viewport.intersect(rect) else {
                continue;
            };
            let area = u64::from(shown.width) * u64::from(shown.height);
            if best.is_none_or(|(_index, most)| area > most) {
                best = Some((index, area));
            }
        }
        best.map(|(index, _area)| index)
    }

    /// The part of the desktop rectangle `rect` that output `index` shows, in
    /// the output's own coordinates (its viewport's top left corner at the
    /// origin).
    #[must_use]
    pub fn local(&self, index: usize, rect: &Rect) -> Option<Rect> {
        let viewport = self.viewport(index)?;
        let shown = viewport.intersect(rect)?;
        Some(Rect {
            x: shown.x.wrapping_sub(viewport.x),
            y: shown.y.wrapping_sub(viewport.y),
            ..shown
        })
    }

    /// The desktop rectangle `rect` in the coordinates of output `index`,
    /// shown or not: to draw a window or the cursor into that output.
    #[must_use]
    pub fn to_local(&self, index: usize, rect: Rect) -> Option<Rect> {
        let viewport = self.viewport(index)?;
        Some(Rect {
            x: rect.x.saturating_sub(viewport.x),
            y: rect.y.saturating_sub(viewport.y),
            ..rect
        })
    }

    /// The point of the union of the viewports nearest to (`x`, `y`): the
    /// point itself when some output shows it. Without outputs the point is
    /// returned as it is.
    #[must_use]
    pub fn clamp(&self, x: i64, y: i64) -> (i32, i32) {
        let mut best: Option<((i32, i32), i64)> = None;
        for (_index, viewport) in self.viewports() {
            if viewport.is_empty() {
                continue;
            }
            let nearest_x = x.clamp(i64::from(viewport.x), viewport.right() - 1);
            let nearest_y = y.clamp(i64::from(viewport.y), viewport.bottom() - 1);
            let distance = (nearest_x - x)
                .saturating_mul(nearest_x - x)
                .saturating_add((nearest_y - y).saturating_mul(nearest_y - y));
            if best.is_none_or(|(_point, nearest)| distance < nearest) {
                // Both lie inside a viewport, whose corners are i32.
                let point = (
                    i32::try_from(nearest_x).unwrap_or(0),
                    i32::try_from(nearest_y).unwrap_or(0),
                );
                best = Some((point, distance));
            }
        }
        best.map_or_else(
            || (i32::try_from(x).unwrap_or(0), i32::try_from(y).unwrap_or(0)),
            |(point, _distance)| point,
        )
    }

    /// Move the pointer at (`x`, `y`) by (`dx`, `dy`): first along x, then
    /// along y, each step clamped onto the union, so that the pointer slides
    /// along an edge where no output continues.
    #[must_use]
    pub fn step(&self, (x, y): (i32, i32), dx: i32, dy: i32) -> (i32, i32) {
        let (x, y) = self.clamp(i64::from(x) + i64::from(dx), i64::from(y));
        self.clamp(i64::from(x), i64::from(y) + i64::from(dy))
    }

    /// The middle of output `index`'s viewport.
    #[must_use]
    pub fn middle(&self, index: usize) -> Option<(i32, i32)> {
        let viewport = self.viewport(index)?;
        Some((
            viewport.x.saturating_add_unsigned(viewport.width / 2),
            viewport.y.saturating_add_unsigned(viewport.height / 2),
        ))
    }

    /// Where a new window of `width` by `height` goes (M10): `offset` pixels
    /// right and down from the top left corner of the output under `pointer`,
    /// or of the first output, moved back left and up as far as needed to fit
    /// that output, but never past its top left corner.
    #[must_use]
    pub fn place(&self, pointer: (i32, i32), width: u32, height: u32, offset: i32) -> (i32, i32) {
        let Some(viewport) = self
            .output_at(pointer.0, pointer.1)
            .and_then(|index| self.viewport(index))
            .or_else(|| self.viewports().next().map(|(_index, viewport)| viewport))
        else {
            return (offset, offset);
        };
        fit(viewport, width, height, offset, offset)
    }

    /// Where a window at `rect` goes when the output at `removed` is taken
    /// away (M10): when no remaining output shows any of it, onto the output
    /// under `pointer` or else the first output, keeping its size and its
    /// place inside the viewport as far as it fits; otherwise it stays
    /// (`None`). The layout is the one without the removed output.
    #[must_use]
    pub fn rehome(&self, rect: Rect, removed: Rect, pointer: (i32, i32)) -> Option<Rect> {
        if self
            .viewports()
            .any(|(_index, viewport)| viewport.intersect(&rect).is_some())
        {
            return None;
        }
        let viewport = self
            .output_at(pointer.0, pointer.1)
            .and_then(|index| self.viewport(index))
            .or_else(|| self.viewports().next().map(|(_index, viewport)| viewport))?;
        let offset_x = i32::try_from(i64::from(rect.x) - i64::from(removed.x)).unwrap_or(0);
        let offset_y = i32::try_from(i64::from(rect.y) - i64::from(removed.y)).unwrap_or(0);
        let (x, y) = fit(viewport, rect.width, rect.height, offset_x, offset_y);
        Some(Rect { x, y, ..rect })
    }

    /// Move every viewport so that the desktop origin is the top left corner
    /// of the output with the lowest x, then the lowest y (D6). Returns how far
    /// everything moved, to move windows and the pointer by.
    pub fn renormalize(&mut self) -> (i32, i32) {
        let Some(anchor) = self
            .viewports()
            .map(|(_index, viewport)| viewport)
            .min_by_key(|viewport| (viewport.x, viewport.y))
        else {
            return (0, 0);
        };
        let (dx, dy) = (anchor.x.saturating_neg(), anchor.y.saturating_neg());
        for viewport in self.viewports.iter_mut().flatten() {
            viewport.x = viewport.x.saturating_add(dx);
            viewport.y = viewport.y.saturating_add(dy);
        }
        (dx, dy)
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

/// The top left corner for `width` by `height` at (`offset_x`, `offset_y`)
/// inside `viewport`, moved left and up as far as needed to fit it, but never
/// past the viewport's top left corner.
fn fit(viewport: Rect, width: u32, height: u32, offset_x: i32, offset_y: i32) -> (i32, i32) {
    let axis = |origin: i32, size: u32, length: u32, offset: i32| {
        let last = i64::from(origin) + i64::from(size) - i64::from(length);
        let wanted = i64::from(origin) + i64::from(offset.max(0));
        i32::try_from(wanted.min(last).max(i64::from(origin))).unwrap_or(origin)
    };
    (
        axis(viewport.x, viewport.width, width, offset_x),
        axis(viewport.y, viewport.height, height, offset_y),
    )
}
