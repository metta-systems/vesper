//! Windows, their rectangles and their stacking order.

/// Windows the compositor manages at once.
pub const MAX_WINDOWS: usize = 16;

/// A rectangle of the screen, in pixels. The origin may lie off the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether the pixel `(x, y)` is inside.
    #[must_use]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        x >= self.x && y >= self.y && i64::from(x) < right && i64::from(y) < bottom
    }

    /// Whether the rectangle covers no pixel.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// One past the rightmost column, as a wide integer.
    #[must_use]
    pub const fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    /// One past the lowest row, as a wide integer.
    #[must_use]
    pub const fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }

    /// The pixels both rectangles cover, if any.
    #[must_use]
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let left = i64::from(self.x.max(other.x));
        let top = i64::from(self.y.max(other.y));
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= left || bottom <= top {
            return None;
        }
        Some(Rect {
            x: i32::try_from(left).ok()?,
            y: i32::try_from(top).ok()?,
            width: u32::try_from(right - left).ok()?,
            height: u32::try_from(bottom - top).ok()?,
        })
    }

    /// Whether the rectangles share a pixel or touch along an edge.
    #[must_use]
    pub fn touches(&self, other: &Rect) -> bool {
        i64::from(self.x) <= other.right()
            && i64::from(other.x) <= self.right()
            && i64::from(self.y) <= other.bottom()
            && i64::from(other.y) <= self.bottom()
    }

    /// The smallest rectangle that holds both. An empty rectangle adds nothing.
    #[must_use]
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect {
            x: left,
            y: top,
            width: u32::try_from(right - i64::from(left)).unwrap_or(u32::MAX),
            height: u32::try_from(bottom - i64::from(top)).unwrap_or(u32::MAX),
        }
    }
}

/// A window: an id the compositor handed out, the client it belongs to and
/// where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub id: u32,
    pub client: u32,
    pub rect: Rect,
}

/// The windows, bottom to top.
#[derive(Clone, Copy, Debug)]
pub struct Windows {
    /// In stacking order, the first `len` of them, bottom first.
    stack: [Option<Window>; MAX_WINDOWS],
    len: usize,
}

impl Windows {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stack: [None; MAX_WINDOWS],
            len: 0,
        }
    }

    /// How many windows there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Add `window` on top. Returns `false` when the table is full or a window
    /// has the id already.
    pub fn add(&mut self, window: Window) -> bool {
        if self.len == MAX_WINDOWS || self.get(window.id).is_some() {
            return false;
        }
        self.stack[self.len] = Some(window);
        self.len += 1;
        true
    }

    /// The window `id`.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Window> {
        self.stack[..self.len]
            .iter()
            .flatten()
            .find(|window| window.id == id)
    }

    fn position(&self, id: u32) -> Option<usize> {
        self.stack[..self.len]
            .iter()
            .position(|window| window.is_some_and(|window| window.id == id))
    }

    /// Remove the window `id`; the ones above it keep their order.
    pub fn remove(&mut self, id: u32) -> bool {
        let Some(at) = self.position(id) else {
            return false;
        };
        self.stack.copy_within(at + 1..self.len, at);
        self.len -= 1;
        self.stack[self.len] = None;
        true
    }

    /// Put the window `id` on top of the others.
    pub fn raise(&mut self, id: u32) -> bool {
        let Some(at) = self.position(id) else {
            return false;
        };
        let window = self.stack[at];
        self.stack.copy_within(at + 1..self.len, at);
        self.stack[self.len - 1] = window;
        true
    }

    /// Move the window `id` so its origin is `(x, y)`.
    pub fn move_to(&mut self, id: u32, x: i32, y: i32) -> bool {
        let Some(at) = self.position(id) else {
            return false;
        };
        if let Some(window) = &mut self.stack[at] {
            window.rect.x = x;
            window.rect.y = y;
        }
        true
    }

    /// The id of the topmost window under the pixel `(x, y)`.
    #[must_use]
    pub fn top_at(&self, x: i32, y: i32) -> Option<u32> {
        self.stack[..self.len]
            .iter()
            .rev()
            .flatten()
            .find(|window| window.rect.contains(x, y))
            .map(|window| window.id)
    }

    /// The windows from the bottom up, for painting.
    pub fn bottom_up(&self) -> impl Iterator<Item = &Window> {
        self.stack[..self.len].iter().flatten()
    }
}

impl Default for Windows {
    fn default() -> Self {
        Self::new()
    }
}
