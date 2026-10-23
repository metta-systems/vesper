//! Tests for `p_maputl.c` pure math functions.
//!
//! These functions need no loaded level: the geometry tests build a small
//! level of their own and pass it in, so they can be called directly
//! against the C implementation.

#![allow(non_snake_case)]

use std::sync::Mutex;

use {
    crate::harness::C_GLOBAL_LOCK,
    room::engine::{
        map::{LineId, SectorId, VertexId, line_t, sector_t, slopetype_t, vertex_t},
        math::{bbox::BBox, fixed::FRACUNIT},
        play::{self, maputl::divline_t, setup::LevelState},
    },
};

// ---------------------------------------------------------------------------
// P_AproxDistance
// ---------------------------------------------------------------------------

/// `P_AproxDistance(0, 0)` is zero — the degenerate case.
#[test]
fn aproxdist_zero() {
    assert_eq!(play::maputl::P_AproxDistance(0, 0), 0);
}

/// Distance along a purely horizontal vector equals `|dx|`.
#[test]
fn aproxdist_along_x() {
    assert_eq!(
        play::maputl::P_AproxDistance(100 * FRACUNIT, 0),
        100 * FRACUNIT
    );
}

/// Distance along a purely vertical vector equals `|dy|`.
#[test]
fn aproxdist_along_y() {
    assert_eq!(
        play::maputl::P_AproxDistance(0, 100 * FRACUNIT),
        100 * FRACUNIT
    );
}

/// 45-degree diagonal exercises the octant approximation `dx + dy - min/2`.
#[test]
fn aproxdist_diagonal() {
    // Octant approximation: dx + dy - min(dx,dy)/2
    // 100 + 100 - 50 = 150
    assert_eq!(
        play::maputl::P_AproxDistance(100 * FRACUNIT, 100 * FRACUNIT),
        150 * FRACUNIT
    );
}

/// A negative `dx` is taken absolute-value before the approximation.
#[test]
fn aproxdist_negative_dx() {
    assert_eq!(
        play::maputl::P_AproxDistance(-100 * FRACUNIT, 0),
        100 * FRACUNIT
    );
}

/// A negative `dy` is taken absolute-value before the approximation.
#[test]
fn aproxdist_negative_dy() {
    assert_eq!(
        play::maputl::P_AproxDistance(0, -100 * FRACUNIT),
        100 * FRACUNIT
    );
}

/// Both components negative; the result depends only on magnitudes.
#[test]
fn aproxdist_both_negative() {
    // dx=100, dy=50 → 100 + 50 - 25 = 125
    assert_eq!(
        play::maputl::P_AproxDistance(-100 * FRACUNIT, -50 * FRACUNIT),
        125 * FRACUNIT
    );
}

/// When `|dx| < |dy|`, the approximation halves `dx` (the smaller magnitude).
#[test]
fn aproxdist_dx_lt_dy() {
    // dx < dy → dx + dy - (dx >> 1)
    // 50 + 100 - 25 = 125
    assert_eq!(
        play::maputl::P_AproxDistance(50 * FRACUNIT, 100 * FRACUNIT),
        125 * FRACUNIT
    );
}

/// When `|dx| > |dy|`, the approximation halves `dy` (the smaller magnitude).
#[test]
fn aproxdist_dx_gt_dy() {
    // dx > dy → dx + dy - (dy >> 1)
    // 100 + 50 - 25 = 125
    assert_eq!(
        play::maputl::P_AproxDistance(100 * FRACUNIT, 50 * FRACUNIT),
        125 * FRACUNIT
    );
}

/// Extremal inputs (`i32::MAX`, `i32::MIN`) must not panic — Doom wraps on overflow.
#[test]
fn aproxdist_max_values_no_panic() {
    let _ = play::maputl::P_AproxDistance(i32::MAX, i32::MAX);
    let _ = play::maputl::P_AproxDistance(i32::MIN, i32::MIN);
}

/// Mixed sign components are taken absolute-value independently.
#[test]
fn aproxdist_mixed_sign() {
    // abs(-64) = 64, abs(128) = 128 → 64 + 128 - 32 = 160
    assert_eq!(
        play::maputl::P_AproxDistance(-64 * FRACUNIT, 128 * FRACUNIT),
        160 * FRACUNIT
    );
}

// ---------------------------------------------------------------------------
// P_PointOnLineSide
//
// Expected values derived from the identical c2rust-intermediate implementation.
//
// C semantics (return 0 = front, 1 = back / on-line):
//   Vertical   (dx == 0): x <= v1.x → (dy > 0),  x > v1.x → (dy < 0)
//   Horizontal (dy == 0): y <= v1.y → (dx < 0),  y > v1.y → (dx > 0)
//   General:  left = FixedMul(dy>>FRACBITS, dx_val)
//             right = FixedMul(dy_val, dx>>FRACBITS)
//             right < left → 0,  else → 1
// ---------------------------------------------------------------------------

/// The id of the only line in the levels these tests build.
const LINE: LineId = LineId::new(0);

/// A minimal `line_t` from vertex 0: only `v1`, `dx`, `dy`, `sidenum`,
/// `slopetype` and the two sectors are meaningful; the remaining fields are
/// zeroed.
fn make_line_record(
    dx: i32,
    dy: i32,
    sidenum: [i16; 2],
    slopetype: slopetype_t,
    frontsector: Option<SectorId>,
    backsector: Option<SectorId>,
) -> line_t {
    line_t {
        v1: VertexId::new(0),
        v2: VertexId::new(0),
        dx,
        dy,
        flags: 0,
        special: 0,
        tag: 0,
        sidenum,
        bbox: [0; 4],
        slopetype,
        frontsector,
        backsector,
        validcount: 0,
        specialdata: None,
    }
}

/// A level holding the single linedef `line` (id [`LINE`]), its first vertex
/// `v1` (vertex 0) and `sectors`.
fn one_line_level(v1: vertex_t, line: line_t, sectors: Vec<sector_t>) -> LevelState {
    LevelState {
        vertexes: vec![v1],
        lines: vec![line],
        numsectors: sectors.len() as i32,
        sectors,
        ..LevelState::new()
    }
}

/// A level for `P_PointOnLineSide` tests: one line from `v1` along
/// `(dx, dy)`.
fn make_pols_level(v1: vertex_t, dx: i32, dy: i32) -> LevelState {
    let line = make_line_record(dx, dy, [-1, -1], slopetype_t::ST_HORIZONTAL, None, None);
    one_line_level(v1, line, Vec::new())
}

/// Vertical line pointing up (`dy > 0`), point to the left → back side (1).
#[test]
fn point_on_line_side_vertical_pos_dy_left() {
    // dx=0, dy>0, x < v1.x → back (1)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, 0, FRACUNIT);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, -1, 0, LINE), 1);
}

/// Vertical line pointing up, point to the right → front side (0).
#[test]
fn point_on_line_side_vertical_pos_dy_right() {
    // dx=0, dy>0, x > v1.x → front (0)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, 0, FRACUNIT);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 1, 0, LINE), 0);
}

/// Vertical line pointing up, point exactly on the line → back side (the
/// `x <= v1.x` branch covers equality).
#[test]
fn point_on_line_side_vertical_pos_dy_on_line() {
    // dx=0, dy>0, x == v1.x (takes x <= v1.x branch) → back (1)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, 0, FRACUNIT);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, 0, LINE), 1);
}

/// Vertical line pointing down (`dy < 0`), point to the left → front side.
#[test]
fn point_on_line_side_vertical_neg_dy_left() {
    // dx=0, dy<0, x <= v1.x → front (0)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, 0, -FRACUNIT);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, -1, 0, LINE), 0);
}

/// Vertical line pointing down, point to the right → back side.
#[test]
fn point_on_line_side_vertical_neg_dy_right() {
    // dx=0, dy<0, x > v1.x → back (1)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, 0, -FRACUNIT);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 1, 0, LINE), 1);
}

/// Horizontal line pointing right (`dx > 0`), point below → front side.
#[test]
fn point_on_line_side_horizontal_pos_dx_below() {
    // dy=0, dx>0, y < v1.y → front (0)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, 0);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, -1, LINE), 0);
}

/// Horizontal line pointing right, point above → back side.
#[test]
fn point_on_line_side_horizontal_pos_dx_above() {
    // dy=0, dx>0, y > v1.y → back (1)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, 0);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, 1, LINE), 1);
}

/// Horizontal line pointing right, point exactly on it → front side (the
/// `y <= v1.y` branch covers equality).
#[test]
fn point_on_line_side_horizontal_pos_dx_on_line() {
    // dy=0, dx>0, y == v1.y (takes y <= v1.y branch) → front (0)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, 0);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, 0, LINE), 0);
}

/// Horizontal line pointing left (`dx < 0`), point below → back side.
#[test]
fn point_on_line_side_horizontal_neg_dx_below() {
    // dy=0, dx<0, y <= v1.y → back (1)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, -FRACUNIT, 0);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, -1, LINE), 1);
}

/// Horizontal line pointing left, point above → front side.
#[test]
fn point_on_line_side_horizontal_neg_dx_above() {
    // dy=0, dx<0, y > v1.y → front (0)
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, -FRACUNIT, 0);
    assert_eq!(play::maputl::P_PointOnLineSide(&level, 0, 1, LINE), 0);
}

/// Diagonal NE line, point above-left → back side via the cross-product branch.
#[test]
fn point_on_line_side_diagonal_above_left() {
    // NE diagonal (dx=dy=FRACUNIT), point at (0, FRACUNIT) — above-left → back (1)
    // left=FixedMul(1,0)=0, right=FixedMul(FRACUNIT,1)=1, 1>=0 → 1
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, FRACUNIT);
    assert_eq!(
        play::maputl::P_PointOnLineSide(&level, 0, FRACUNIT, LINE),
        1
    );
}

/// Diagonal NE line, point below-right → front side via the cross-product branch.
#[test]
fn point_on_line_side_diagonal_below_right() {
    // NE diagonal (dx=dy=FRACUNIT), point at (FRACUNIT, 0) — below-right → front (0)
    // left=FixedMul(1,FRACUNIT)=1, right=FixedMul(0,1)=0, 0<1 → 0
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, FRACUNIT);
    assert_eq!(
        play::maputl::P_PointOnLineSide(&level, FRACUNIT, 0, LINE),
        0
    );
}

/// Diagonal NE line, point exactly on it: `right == left` returns back side
/// (the comparison is `right < left → front`, equality falls through to back).
#[test]
fn point_on_line_side_diagonal_on_line_is_back() {
    // Point exactly on NE diagonal: right==left → back (1)
    // left=FixedMul(1,FRACUNIT)=1, right=FixedMul(FRACUNIT,1)=1, 1>=1 → 1
    let level = make_pols_level(vertex_t { x: 0, y: 0 }, FRACUNIT, FRACUNIT);
    assert_eq!(
        play::maputl::P_PointOnLineSide(&level, FRACUNIT, FRACUNIT, LINE),
        1
    );
}

// ---------------------------------------------------------------------------
// P_PointOnDivlineSide
//
// C semantics:
//   Horizontal (dy == 0):  y > line.y  → side 1 (back)
//                           y <= line.y → side 0 (front)
//   Vertical   (dx == 0):  x <= line.x → side 1 (back)
//                           x > line.x  → side 0 (front)
// ---------------------------------------------------------------------------

/// Build a `divline_t` from explicit origin and direction components.
fn make_divline(x: i32, y: i32, dx: i32, dy: i32) -> divline_t {
    divline_t { x, y, dx, dy }
}

/// Horizontal divline (pointing right), point above → back side (1).
#[test]
fn divline_side_horizontal_above() {
    let line = make_divline(0, 0, FRACUNIT, 0); // pointing right
    // Above the line → side 1 (back)
    assert_eq!(
        play::maputl::P_PointOnDivlineSide(FRACUNIT, FRACUNIT, &line),
        1
    );
}

/// Horizontal divline, point below → front side (0).
#[test]
fn divline_side_horizontal_below() {
    let line = make_divline(0, 0, FRACUNIT, 0);
    // Below the line → side 0 (front)
    assert_eq!(
        play::maputl::P_PointOnDivlineSide(FRACUNIT, -FRACUNIT, &line),
        0
    );
}

/// Horizontal divline, point exactly on it → front side (`y <= line.y`).
#[test]
fn divline_side_horizontal_on_line() {
    let line = make_divline(0, 0, FRACUNIT, 0);
    // y <= line.y → side 0
    assert_eq!(play::maputl::P_PointOnDivlineSide(FRACUNIT, 0, &line), 0);
}

/// Vertical divline (pointing up), point to the left → back side (1).
#[test]
fn divline_side_vertical_left() {
    let line = make_divline(0, 0, 0, FRACUNIT); // pointing up
    // Left of the line (x <= 0) → side 1 (back)
    assert_eq!(
        play::maputl::P_PointOnDivlineSide(-FRACUNIT, FRACUNIT, &line),
        1
    );
}

/// Vertical divline, point to the right → front side (0).
#[test]
fn divline_side_vertical_right() {
    let line = make_divline(0, 0, 0, FRACUNIT);
    // Right of the line (x > 0) → side 0 (front)
    assert_eq!(
        play::maputl::P_PointOnDivlineSide(FRACUNIT, FRACUNIT, &line),
        0
    );
}

/// Vertical divline, point exactly on it → back side (`x <= line.x`).
#[test]
fn divline_side_vertical_on_line() {
    let line = make_divline(0, 0, 0, FRACUNIT);
    // x <= line.x → side 1
    assert_eq!(play::maputl::P_PointOnDivlineSide(0, FRACUNIT, &line), 1);
}

/// Diagonal divline (`y = x`): points are tested on either side using the
/// general cross-product branch.
#[test]
fn divline_side_diagonal() {
    // 45-degree line y = x
    let line = make_divline(0, 0, FRACUNIT, FRACUNIT);
    // point (0, FRACUNIT) is above the line → side 1
    assert_eq!(play::maputl::P_PointOnDivlineSide(0, FRACUNIT, &line), 1);
    // point (FRACUNIT, 0) is below the line → side 0
    assert_eq!(play::maputl::P_PointOnDivlineSide(FRACUNIT, 0, &line), 0);
}

// ---------------------------------------------------------------------------
// P_InterceptVector
//
// Returns the fractional intercept point along the FIRST divline (v2).
// ---------------------------------------------------------------------------

/// Two parallel horizontal divlines never intersect — the function returns 0
/// rather than dividing by zero.
#[test]
fn intercept_parallel_returns_zero() {
    let v1 = make_divline(0, 0, FRACUNIT, 0);
    let v2 = make_divline(0, FRACUNIT, FRACUNIT, 0);
    // Parallel horizontal lines → 0
    assert_eq!(play::maputl::P_InterceptVector(&v2, &v1), 0);
}

/// Perpendicular divlines crossing at the origin → intercept fraction is 0
/// (the crossing is at the start of `v2`).
#[test]
fn intercept_perpendicular_crossing_at_origin() {
    let v1 = make_divline(0, 0, FRACUNIT, 0); // x-axis
    let v2 = make_divline(0, 0, 0, FRACUNIT); // y-axis
    // They cross at origin → frac = 0 along v2
    assert_eq!(play::maputl::P_InterceptVector(&v2, &v1), 0);
}

/// Horizontal `v1` crossed at the midpoint of vertical `v2` → intercept = 0.5.
#[test]
fn intercept_crossing_at_half_along_v2() {
    // v1: from (0,0) to (1,0)  — horizontal
    // v2: from (1,-1) to (1,1) — vertical through x=1
    let v1 = make_divline(0, 0, FRACUNIT, 0);
    let v2 = make_divline(FRACUNIT, -FRACUNIT, 0, 2 * FRACUNIT);
    // Intersection is at (1,0), which is halfway along v2
    // → frac = 0.5 = FRACUNIT/2
    assert_eq!(play::maputl::P_InterceptVector(&v2, &v1), FRACUNIT / 2);
}

/// Same midpoint crossing but with a longer `v1` — the intercept is
/// expressed as a fraction along `v2`, so the result is unchanged.
#[test]
fn intercept_crossing_at_one_along_v2() {
    // v1: from (0,0) to (2,0)
    // v2: from (1,-1) to (1,1)
    let v1 = make_divline(0, 0, 2 * FRACUNIT, 0);
    let v2 = make_divline(FRACUNIT, -FRACUNIT, 0, 2 * FRACUNIT);
    // Intersection at (1,0) is halfway along v2 → 0.5
    assert_eq!(play::maputl::P_InterceptVector(&v2, &v1), FRACUNIT / 2);
}

// ---------------------------------------------------------------------------
// P_MakeDivline
// ---------------------------------------------------------------------------

/// `P_MakeDivline` should copy `(v1.x, v1.y, dx, dy)` from a `line_t` into
/// the supplied `divline_t` unchanged.
#[test]
fn make_divline_copies_fields() {
    let v = vertex_t {
        x: 10 * FRACUNIT,
        y: 20 * FRACUNIT,
    };
    let line = make_line_record(
        5 * FRACUNIT,
        7 * FRACUNIT,
        [-1, -1],
        slopetype_t::ST_HORIZONTAL,
        None,
        None,
    );
    let level = one_line_level(v, line, Vec::new());
    let mut dl = divline_t {
        x: 0,
        y: 0,
        dx: 0,
        dy: 0,
    };
    play::maputl::P_MakeDivline(&level, LINE, &mut dl);
    assert_eq!(dl.x, 10 * FRACUNIT);
    assert_eq!(dl.y, 20 * FRACUNIT);
    assert_eq!(dl.dx, 5 * FRACUNIT);
    assert_eq!(dl.dy, 7 * FRACUNIT);
}

// ---------------------------------------------------------------------------
// P_BoxOnLineSide
// ---------------------------------------------------------------------------

/// Horizontal `ST_HORIZONTAL` line, bounding box entirely above → back side.
#[test]
fn box_on_line_horizontal_above() {
    let line = make_line_record(
        FRACUNIT,
        0,
        [-1, -1],
        slopetype_t::ST_HORIZONTAL,
        None,
        None,
    );
    let level = one_line_level(vertex_t { x: 0, y: 0 }, line, Vec::new());
    let mut tmbox = [0i32; 4];
    // Box entirely above the line → back side = 1
    tmbox[BBox::TOP] = FRACUNIT;
    tmbox[BBox::BOTTOM] = FRACUNIT / 2;
    assert_eq!(play::maputl::P_BoxOnLineSide(&level, &tmbox, LINE), 1);
}

/// Horizontal `ST_HORIZONTAL` line, bounding box entirely below → front side.
#[test]
fn box_on_line_horizontal_below() {
    let line = make_line_record(
        FRACUNIT,
        0,
        [-1, -1],
        slopetype_t::ST_HORIZONTAL,
        None,
        None,
    );
    let level = one_line_level(vertex_t { x: 0, y: 0 }, line, Vec::new());
    let mut tmbox = [0i32; 4];
    // Box entirely below the line → front side = 0
    tmbox[BBox::TOP] = -FRACUNIT / 2;
    tmbox[BBox::BOTTOM] = -FRACUNIT;
    assert_eq!(play::maputl::P_BoxOnLineSide(&level, &tmbox, LINE), 0);
}

/// Vertical `ST_VERTICAL` line, bounding box entirely to the right → front side.
#[test]
fn box_on_line_vertical_right() {
    let line = make_line_record(0, FRACUNIT, [-1, -1], slopetype_t::ST_VERTICAL, None, None);
    let level = one_line_level(vertex_t { x: 0, y: 0 }, line, Vec::new());
    let mut tmbox = [0i32; 4];
    // Box entirely to the right (x > 0) → front side = 0
    tmbox[BBox::RIGHT] = FRACUNIT;
    tmbox[BBox::LEFT] = FRACUNIT / 2;
    assert_eq!(play::maputl::P_BoxOnLineSide(&level, &tmbox, LINE), 0);
}

/// Vertical `ST_VERTICAL` line, bounding box entirely to the left → back side.
#[test]
fn box_on_line_vertical_left() {
    let line = make_line_record(0, FRACUNIT, [-1, -1], slopetype_t::ST_VERTICAL, None, None);
    let level = one_line_level(vertex_t { x: 0, y: 0 }, line, Vec::new());
    let mut tmbox = [0i32; 4];
    // Box entirely to the left (x <= 0) → back side = 1
    tmbox[BBox::RIGHT] = -FRACUNIT / 2;
    tmbox[BBox::LEFT] = -FRACUNIT;
    assert_eq!(play::maputl::P_BoxOnLineSide(&level, &tmbox, LINE), 1);
}

/// When the bounding box straddles the line, `P_BoxOnLineSide` returns -1
/// (the box is on **both** sides — neither front nor back).
#[test]
fn box_crosses_line_returns_negative_one() {
    let line = make_line_record(
        FRACUNIT,
        0,
        [-1, -1],
        slopetype_t::ST_HORIZONTAL,
        None,
        None,
    );
    let level = one_line_level(vertex_t { x: 0, y: 0 }, line, Vec::new());
    let mut tmbox = [0i32; 4];
    // Box straddles the line (top above, bottom below)
    tmbox[BBox::TOP] = FRACUNIT;
    tmbox[BBox::BOTTOM] = -FRACUNIT;
    assert_eq!(play::maputl::P_BoxOnLineSide(&level, &tmbox, LINE), -1);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Zero-initialise a `sector_t` and set only the two height fields the
/// line-opening tests care about.
fn make_sector(floorheight: i32, ceilingheight: i32) -> sector_t {
    let mut s = sector_t::ZERO;
    s.floorheight = floorheight;
    s.ceilingheight = ceilingheight;
    s
}

/// Build a level with one horizontal line with the supplied sides for
/// `P_LineOpening` tests: its front sector is `sectors[0]` and its back
/// sector `sectors[1]`, where present.  `v2`, special, tag, bbox, and
/// validcount are not exercised by the function under test.
fn make_opening_level(sidenum: [i16; 2], sectors: Vec<sector_t>) -> LevelState {
    let sector = |index: usize| (index < sectors.len()).then(|| SectorId::new(index));
    let line = make_line_record(
        FRACUNIT,
        0,
        sidenum,
        slopetype_t::ST_HORIZONTAL,
        sector(0),
        sector(1),
    );
    one_line_level(vertex_t { x: 0, y: 0 }, line, sectors)
}

// ---------------------------------------------------------------------------
// P_LineOpening
// ---------------------------------------------------------------------------

/// Single-sided lines (back side `-1`) leave `openrange = 0` — players and
/// monsters can never pass through them.
#[test]
fn line_opening_single_sided() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    let level = make_opening_level([0, -1], Vec::new());
    play::maputl::P_LineOpening(&level, LINE);
    assert_eq!(play::maputl::openrange(), 0);
}

/// Two-sided line where the front sector has a higher ceiling: opening
/// extends from `max(floors)` to `min(ceilings)`, `lowfloor` is the back floor.
#[test]
fn line_opening_two_sided_front_higher() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    let front = make_sector(0, 128 * FRACUNIT);
    let back = make_sector(-16 * FRACUNIT, 64 * FRACUNIT);
    let level = make_opening_level([0, 1], vec![front, back]);
    play::maputl::P_LineOpening(&level, LINE);
    // opentop    = min(128, 64)  = 64
    // openbottom = max(0, -16)   = 0
    // lowfloor   = min(0, -16)   = -16
    // openrange  = 64 - 0        = 64
    assert_eq!(play::maputl::opentop(), 64 * FRACUNIT);
    assert_eq!(play::maputl::openbottom(), 0);
    assert_eq!(play::maputl::lowfloor(), -16 * FRACUNIT);
    assert_eq!(play::maputl::openrange(), 64 * FRACUNIT);
}

/// Two-sided line where the back sector has a higher floor: opening starts
/// at the back floor, `lowfloor` becomes the front floor.
#[test]
fn line_opening_two_sided_back_higher() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    let front = make_sector(0, 64 * FRACUNIT);
    let back = make_sector(16 * FRACUNIT, 128 * FRACUNIT);
    let level = make_opening_level([0, 1], vec![front, back]);
    play::maputl::P_LineOpening(&level, LINE);
    // opentop    = min(64, 128)  = 64
    // openbottom = max(0, 16)    = 16
    // lowfloor   = min(0, 16)    = 0
    // openrange  = 64 - 16       = 48
    assert_eq!(play::maputl::opentop(), 64 * FRACUNIT);
    assert_eq!(play::maputl::openbottom(), 16 * FRACUNIT);
    assert_eq!(play::maputl::lowfloor(), 0);
    assert_eq!(play::maputl::openrange(), 48 * FRACUNIT);
}

/// Two-sided line with identical sectors on both sides: opening equals the
/// full sector height, `lowfloor` equals `openbottom`.
#[test]
fn line_opening_two_sided_equal() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    let front = make_sector(32 * FRACUNIT, 128 * FRACUNIT);
    let back = make_sector(32 * FRACUNIT, 128 * FRACUNIT);
    let level = make_opening_level([0, 1], vec![front, back]);
    play::maputl::P_LineOpening(&level, LINE);
    assert_eq!(play::maputl::opentop(), 128 * FRACUNIT);
    assert_eq!(play::maputl::openbottom(), 32 * FRACUNIT);
    assert_eq!(play::maputl::lowfloor(), 32 * FRACUNIT);
    assert_eq!(play::maputl::openrange(), 96 * FRACUNIT);
}

/// Edge case: front floor sits above back ceiling, producing a negative
/// `openrange` — interpreted by callers as "no opening at all".
#[test]
fn line_opening_negative_range() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    // front floor above back ceiling → negative openrange
    let front = make_sector(100 * FRACUNIT, 128 * FRACUNIT);
    let back = make_sector(0, 64 * FRACUNIT);
    let level = make_opening_level([0, 1], vec![front, back]);
    play::maputl::P_LineOpening(&level, LINE);
    // opentop    = min(128, 64)   = 64
    // openbottom = max(100, 0)    = 100
    // lowfloor   = min(100, 0)    = 0
    // openrange  = 64 - 100       = -36
    assert_eq!(play::maputl::opentop(), 64 * FRACUNIT);
    assert_eq!(play::maputl::openbottom(), 100 * FRACUNIT);
    assert_eq!(play::maputl::lowfloor(), 0);
    assert_eq!(play::maputl::openrange(), -36 * FRACUNIT);
}

// ---------------------------------------------------------------------------
// P_TraverseIntercepts
// ---------------------------------------------------------------------------

/// Per-test scratch buffer used by the traverse-callback to record the
/// fractions it was invoked with, in invocation order.
static TRAVERSED_FRACS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Test callback: records the intercept fraction and returns 1 to ask
/// `P_TraverseIntercepts` to continue iterating.
///
/// # Safety
///
/// `intr` must be a valid, non-null, properly aligned pointer to an
/// `intercept_t` whose `frac` field is initialised. Only called from
/// within `P_TraverseIntercepts`, which satisfies these requirements.
unsafe fn record_and_continue(intr: *mut play::maputl::intercept_t) -> u32 {
    TRAVERSED_FRACS
        .lock()
        .unwrap()
        .push(unsafe { (*intr).frac });
    1 // continue
}

/// Test callback: records the intercept fraction and returns 0 to make
/// `P_TraverseIntercepts` stop after this single intercept.
///
/// # Safety
///
/// `intr` must be a valid, non-null, properly aligned pointer to an
/// `intercept_t` whose `frac` field is initialised. Only called from
/// within `P_TraverseIntercepts`, which satisfies these requirements.
unsafe fn record_and_stop(intr: *mut play::maputl::intercept_t) -> u32 {
    TRAVERSED_FRACS
        .lock()
        .unwrap()
        .push(unsafe { (*intr).frac });
    0 // stop
}

/// Fill the first intercepts with `fracs` (line intercepts of line 0) and
/// point the write cursor just past them, as `P_PathTraverse` leaves it.
fn load_intercepts(fracs: &[i32]) {
    {
        let mut intercepts = play::maputl::INTERCEPTS.borrow_mut();
        for (slot, &frac) in intercepts.iter_mut().zip(fracs) {
            *slot = play::maputl::intercept_t {
                frac,
                hit: play::maputl::InterceptHit::Line(room::engine::map::LineId::new(0)),
            };
        }
    }
    let base = play::maputl::INTERCEPTS
        .as_ptr()
        .cast::<play::maputl::intercept_t>();
    // SAFETY: `fracs` fits in the array, so the cursor stays in bounds.
    play::maputl::MAPUTL.borrow_mut().intercept_p = unsafe { base.add(fracs.len()) };
}

/// Empty intercept list → traversal returns `true` immediately without
/// invoking the callback.
#[test]
fn traverse_intercepts_empty() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    TRAVERSED_FRACS.lock().unwrap().clear();
    load_intercepts(&[]);
    // SAFETY: the callback only reads `frac` from the loaded intercepts.
    let result = unsafe { play::maputl::P_TraverseIntercepts(Some(record_and_continue), FRACUNIT) };
    assert_eq!(result, 1); // true = all traversed
    assert!(TRAVERSED_FRACS.lock().unwrap().is_empty());
}

/// Single intercept within `maxfrac` is delivered exactly once.
#[test]
fn traverse_intercepts_single() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    TRAVERSED_FRACS.lock().unwrap().clear();
    load_intercepts(&[FRACUNIT / 2]);

    // SAFETY: the callback only reads `frac` from the loaded intercepts.
    let result = unsafe { play::maputl::P_TraverseIntercepts(Some(record_and_continue), FRACUNIT) };
    assert_eq!(result, 1);
    let fracs = TRAVERSED_FRACS.lock().unwrap();
    assert_eq!(fracs.len(), 1);
    assert_eq!(fracs[0], FRACUNIT / 2);
}

/// Out-of-order intercepts are delivered to the callback in ascending
/// `frac` order — `P_TraverseIntercepts` selects the smallest unprocessed
/// intercept on each iteration.
#[test]
fn traverse_intercepts_sorted_order() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    TRAVERSED_FRACS.lock().unwrap().clear();

    // Fill 3 intercepts with fracs out of order (all <= FRACUNIT).
    load_intercepts(&[FRACUNIT / 2, FRACUNIT / 4, 3 * FRACUNIT / 4]);

    // SAFETY: the callback only reads `frac` from the loaded intercepts.
    let result = unsafe { play::maputl::P_TraverseIntercepts(Some(record_and_continue), FRACUNIT) };
    assert_eq!(result, 1);
    let fracs = TRAVERSED_FRACS.lock().unwrap();
    assert_eq!(fracs.len(), 3);
    assert_eq!(fracs[0], FRACUNIT / 4);
    assert_eq!(fracs[1], FRACUNIT / 2);
    assert_eq!(fracs[2], 3 * FRACUNIT / 4);
}

/// Intercepts whose `frac` exceeds `maxfrac` are skipped — only those in
/// range invoke the callback.
#[test]
fn traverse_intercepts_respects_maxfrac() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    TRAVERSED_FRACS.lock().unwrap().clear();

    load_intercepts(&[FRACUNIT / 4, FRACUNIT]);

    // maxfrac = FRACUNIT/2 → only the first intercept (FRACUNIT/4) is in range.
    // SAFETY: the callback only reads `frac` from the loaded intercepts.
    let result =
        unsafe { play::maputl::P_TraverseIntercepts(Some(record_and_continue), FRACUNIT / 2) };
    assert_eq!(result, 1);
    let fracs = TRAVERSED_FRACS.lock().unwrap();
    assert_eq!(fracs.len(), 1);
    assert_eq!(fracs[0], FRACUNIT / 4);
}

/// Callback returning 0 stops traversal after the current intercept —
/// `P_TraverseIntercepts` returns `false` to signal early termination.
#[test]
fn traverse_intercepts_stop_early() {
    let _guard = C_GLOBAL_LOCK.lock().unwrap();
    TRAVERSED_FRACS.lock().unwrap().clear();

    load_intercepts(&[FRACUNIT / 4, FRACUNIT / 2]);

    // SAFETY: the callback only reads `frac` from the loaded intercepts.
    let result = unsafe { play::maputl::P_TraverseIntercepts(Some(record_and_stop), FRACUNIT) };
    assert_eq!(result, 0); // false = stopped early
    let fracs = TRAVERSED_FRACS.lock().unwrap();
    assert_eq!(fracs.len(), 1);
    assert_eq!(fracs[0], FRACUNIT / 4);
}
