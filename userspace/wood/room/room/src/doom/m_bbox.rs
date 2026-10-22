//! Rust port of vendor/doomgeneric/m_bbox.c.
//!
//! Bounding-box helpers: `M_ClearBox` initialises an inverted box and
//! `M_AddToBox` expands it to contain a point. Boxes are flat
//! `[fixed_t; 4]` arrays indexed by [`BBox::TOP`] / `BBox::BOTTOM` /
//! `BBox::LEFT` / `BBox::RIGHT`, matching the `BOXTOP` / `BOXBOTTOM` /
//! `BOXLEFT` / `BOXRIGHT` enum in `m_bbox.h`.

#![allow(non_snake_case)]

use super::m_fixed::fixed_t;

/// Indices into a 4-element bounding-box array, matching `m_bbox.h`.
pub struct BBox;

/// Constants exposing the bbox slot indices. Kept as `usize` so they can
/// be used directly to index a `[fixed_t; 4]` without further casts.
impl BBox {
    /// Top edge slot (largest y). Matches `BOXTOP`.
    #[doc(alias = "BOXTOP")]
    pub const TOP: usize = 0;
    /// Bottom edge slot (smallest y). Matches `BOXBOTTOM`.
    #[doc(alias = "BOXBOTTOM")]
    pub const BOTTOM: usize = 1;
    /// Left edge slot (smallest x). Matches `BOXLEFT`.
    #[doc(alias = "BOXLEFT")]
    pub const LEFT: usize = 2;
    /// Right edge slot (largest x). Matches `BOXRIGHT`.
    #[doc(alias = "BOXRIGHT")]
    pub const RIGHT: usize = 3;
}

/// `void M_ClearBox(fixed_t *box)` — initialises an inverted bbox so
/// subsequent `M_AddToBox` calls shrink it to fit.
///
/// # Safety
/// `bbox` must point to at least 4 contiguous `fixed_t` values.
#[no_mangle]
pub unsafe extern "C" fn M_ClearBox(bbox: *mut fixed_t) {
    let slice = std::slice::from_raw_parts_mut(bbox, 4);
    slice[BBox::TOP] = i32::MIN;
    slice[BBox::RIGHT] = i32::MIN;
    slice[BBox::BOTTOM] = i32::MAX;
    slice[BBox::LEFT] = i32::MAX;
}

/// `void M_AddToBox(fixed_t *box, fixed_t x, fixed_t y)`.
///
/// # Safety
/// `bbox` must point to at least 4 contiguous `fixed_t` values.
#[no_mangle]
pub unsafe extern "C" fn M_AddToBox(bbox: *mut fixed_t, x: fixed_t, y: fixed_t) {
    let slice = std::slice::from_raw_parts_mut(bbox, 4);
    if x < slice[BBox::LEFT] {
        slice[BBox::LEFT] = x;
    } else if x > slice[BBox::RIGHT] {
        slice[BBox::RIGHT] = x;
    }
    if y < slice[BBox::BOTTOM] {
        slice[BBox::BOTTOM] = y;
    } else if y > slice[BBox::TOP] {
        slice[BBox::TOP] = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that after `M_ClearBox` followed by a single `M_AddToBox`
    /// the box collapses to a point in the LEFT/BOTTOM slots while the
    /// RIGHT/TOP slots stay inverted (an artefact of the C `if`/`else if`).
    #[test]
    fn clear_then_add_shrinks_to_point() {
        let mut bbox: [fixed_t; 4] = [0; 4];
        unsafe {
            M_ClearBox(bbox.as_mut_ptr());
            M_AddToBox(bbox.as_mut_ptr(), 10, 20);
        }
        // BBox::TOP=0, BBox::BOTTOM=1, BBox::LEFT=2, BBox::RIGHT=3
        // After M_ClearBox: [INT_MIN, INT_MAX, INT_MAX, INT_MIN]
        // M_AddToBox(10, 20):
        //   x=10 < BBox::LEFT(INT_MAX) → true → LEFT=10, skips else-if (RIGHT stays INT_MIN)
        //   y=20 < BBox::BOTTOM(INT_MAX) → true → BOTTOM=20, skips else-if (TOP stays INT_MIN)
        assert_eq!(bbox[0], i32::MIN); // top still inverted (else-if branch)
        assert_eq!(bbox[1], 20); // bottom = y
        assert_eq!(bbox[2], 10); // left = x
        assert_eq!(bbox[3], i32::MIN); // right still inverted (else-if branch)
    }

    /// Verifies that successive `M_AddToBox` calls correctly expand the
    /// box outward in all four directions.
    #[test]
    fn add_multiple_points_expands_box() {
        let mut bbox: [fixed_t; 4] = [0; 4];
        unsafe {
            M_ClearBox(bbox.as_mut_ptr());
            M_AddToBox(bbox.as_mut_ptr(), 10, 20);
            M_AddToBox(bbox.as_mut_ptr(), -5, 100);
            M_AddToBox(bbox.as_mut_ptr(), 50, 5);
        }
        assert_eq!(bbox[0], 100); // top
        assert_eq!(bbox[1], 5); // bottom
        assert_eq!(bbox[2], -5); // left
        assert_eq!(bbox[3], 50); // right
    }
}
