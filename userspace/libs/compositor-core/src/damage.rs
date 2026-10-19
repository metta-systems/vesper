//! The part of the screen that has to be drawn again.

use crate::windows::Rect;

/// Rectangles the set holds before it merges them.
pub const MAX_DAMAGE: usize = 8;

/// A bounded set of damaged rectangles. A rectangle that touches one in the set
/// is merged into it, and when the set is full every rectangle becomes their
/// bounding box, so the set never grows and a frame never allocates.
#[derive(Clone, Copy, Debug)]
pub struct Damage {
    rects: [Rect; MAX_DAMAGE],
    len: usize,
}

const NONE: Rect = Rect {
    x: 0,
    y: 0,
    width: 0,
    height: 0,
};

impl Damage {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rects: [NONE; MAX_DAMAGE],
            len: 0,
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The damaged rectangles; none of them is empty.
    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects[..self.len]
    }

    /// Forget all damage.
    pub const fn clear(&mut self) {
        self.len = 0;
    }

    /// Add `rect`; an empty one adds nothing.
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let mut merged = rect;
        // Absorb every rectangle that touches the one being added; the union
        // can reach more of them, so look again until none does.
        while let Some(index) = self.rects[..self.len]
            .iter()
            .position(|held| held.touches(&merged))
        {
            merged = merged.union(&self.rects[index]);
            self.len -= 1;
            self.rects[index] = self.rects[self.len];
        }
        if self.len == MAX_DAMAGE {
            for held in &self.rects[..self.len] {
                merged = merged.union(held);
            }
            self.len = 0;
        }
        self.rects[self.len] = merged;
        self.len += 1;
    }
}

impl Default for Damage {
    fn default() -> Self {
        Self::new()
    }
}
