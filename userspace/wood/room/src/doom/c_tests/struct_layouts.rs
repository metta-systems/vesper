//! Verify every critical struct field offset with `offset_of!`.
//!
//! These tests use `#[repr(C)]` mirrors of the C structs.  If a field
//! offset does not match the C compiler output, the test fails,
//! catching data-type-size and alignment mismatches immediately.
//!
//! Coverage: `vertex_t`, `divline_t`, `line_t`, `sector_t`, `intercept_t`,
//! `mobj_t`, `side_t`, `seg_t`, `node_t`, `drawseg_t`, `vissprite_t`.

#![allow(non_snake_case)]

use std::mem::{offset_of, size_of};

use crate::doom::c_ffi::{
    divline_t, drawseg_t, intercept_t, line_t, mobj_t, node_t, sector_t, seg_t, side_t, vertex_t,
    vissprite_t,
};

// ---------------------------------------------------------------------------
// vertex_t
// ---------------------------------------------------------------------------

/// `vertex_t` is exactly 8 bytes (two `fixed_t` coordinates, no padding).
#[test]
fn vertex_t_size() {
    assert_eq!(size_of::<vertex_t>(), 8);
}

/// `vertex_t` fields: `x` at offset 0, `y` at offset 4.
#[test]
fn vertex_t_offsets() {
    assert_eq!(offset_of!(vertex_t, x), 0);
    assert_eq!(offset_of!(vertex_t, y), 4);
}

// ---------------------------------------------------------------------------
// divline_t
// ---------------------------------------------------------------------------

/// `divline_t` is exactly 16 bytes (origin x/y plus delta dx/dy, all `fixed_t`).
#[test]
fn divline_t_size() {
    assert_eq!(size_of::<divline_t>(), 16);
}

/// `divline_t` fields lay out as `x` (0), `y` (4), `dx` (8), `dy` (12).
#[test]
fn divline_t_offsets() {
    assert_eq!(offset_of!(divline_t, x), 0);
    assert_eq!(offset_of!(divline_t, y), 4);
    assert_eq!(offset_of!(divline_t, dx), 8);
    assert_eq!(offset_of!(divline_t, dy), 12);
}

// ---------------------------------------------------------------------------
// line_t
// ---------------------------------------------------------------------------

/// `line_t` is 88 bytes on a 64-bit target (two 8-byte vertex pointers,
/// fixed-point deltas, sidedef/sector pointers, bbox, and validcount).
#[test]
fn line_t_size() {
    assert_eq!(size_of::<line_t>(), 88);
}

/// `line_t` field offsets, computed for the LP64 layout (matches gcc x86_64).
#[test]
fn line_t_offsets() {
    assert_eq!(offset_of!(line_t, v1), 0);
    assert_eq!(offset_of!(line_t, v2), 8);
    assert_eq!(offset_of!(line_t, dx), 16);
    assert_eq!(offset_of!(line_t, dy), 20);
    assert_eq!(offset_of!(line_t, flags), 24);
    assert_eq!(offset_of!(line_t, special), 26);
    assert_eq!(offset_of!(line_t, tag), 28);
    assert_eq!(offset_of!(line_t, sidenum), 30);
    assert_eq!(offset_of!(line_t, bbox), 36);
    assert_eq!(offset_of!(line_t, slopetype), 52);
    assert_eq!(offset_of!(line_t, frontsector), 56);
    assert_eq!(offset_of!(line_t, backsector), 64);
    assert_eq!(offset_of!(line_t, validcount), 72);
    assert_eq!(offset_of!(line_t, specialdata), 80);
}

// ---------------------------------------------------------------------------
// sector_t
// ---------------------------------------------------------------------------

/// `sector_t` is 128 bytes (heights, picnums, tags, blockbox, validcount,
/// thinglist, specialdata, and line list pointer).
#[test]
fn sector_t_size() {
    assert_eq!(size_of::<sector_t>(), 128);
}

/// `sector_t` field offsets for the LP64 layout.
#[test]
fn sector_t_offsets() {
    assert_eq!(offset_of!(sector_t, floorheight), 0);
    assert_eq!(offset_of!(sector_t, ceilingheight), 4);
    assert_eq!(offset_of!(sector_t, floorpic), 8);
    assert_eq!(offset_of!(sector_t, ceilingpic), 10);
    assert_eq!(offset_of!(sector_t, lightlevel), 12);
    assert_eq!(offset_of!(sector_t, special), 14);
    assert_eq!(offset_of!(sector_t, tag), 16);
    assert_eq!(offset_of!(sector_t, soundtraversed), 20);
    assert_eq!(offset_of!(sector_t, soundtarget), 24);
    assert_eq!(offset_of!(sector_t, blockbox), 32);
    assert_eq!(offset_of!(sector_t, validcount), 88);
    assert_eq!(offset_of!(sector_t, thinglist), 96);
    assert_eq!(offset_of!(sector_t, specialdata), 104);
    assert_eq!(offset_of!(sector_t, linecount), 112);
    assert_eq!(offset_of!(sector_t, lines), 120);
}

// ---------------------------------------------------------------------------
// intercept_t
// ---------------------------------------------------------------------------

/// `intercept_t` is 16 bytes (frac + isaline + union `d`).
#[test]
fn intercept_t_size() {
    assert_eq!(size_of::<intercept_t>(), 16);
}

/// `intercept_t` exposes `frac` at offset 0 and `isaline` at offset 4; the
/// union `d` begins at offset 8.
#[test]
fn intercept_t_offsets() {
    assert_eq!(offset_of!(intercept_t, frac), 0);
    assert_eq!(offset_of!(intercept_t, isaline), 4);
    // union d starts at offset 8
}

// ---------------------------------------------------------------------------
// mobj_t
// ---------------------------------------------------------------------------

/// `mobj_t` is 224 bytes on LP64 (24-byte thinker_t prefix plus the body).
#[test]
fn mobj_t_size() {
    assert_eq!(size_of::<mobj_t>(), 224);
}

/// `mobj_t` field offsets — `x`/`y`/`z` start at 24 (after the thinker prefix);
/// remaining offsets follow the d_player.h / p_mobj.h layout exactly.
#[test]
fn mobj_t_offsets() {
    assert_eq!(offset_of!(mobj_t, x), 24);
    assert_eq!(offset_of!(mobj_t, y), 28);
    assert_eq!(offset_of!(mobj_t, z), 32);
    assert_eq!(offset_of!(mobj_t, angle), 56);
    assert_eq!(offset_of!(mobj_t, radius), 104);
    assert_eq!(offset_of!(mobj_t, height), 108);
    assert_eq!(offset_of!(mobj_t, type_), 128);
    assert_eq!(offset_of!(mobj_t, state), 152);
    assert_eq!(offset_of!(mobj_t, flags), 160);
    assert_eq!(offset_of!(mobj_t, health), 164);
    assert_eq!(offset_of!(mobj_t, reactiontime), 184);
    assert_eq!(offset_of!(mobj_t, player), 192);
    assert_eq!(offset_of!(mobj_t, tracer), 216);
}

// ---------------------------------------------------------------------------
// side_t  (r_defs.h — used by r_segs.c and many others)
// ---------------------------------------------------------------------------

/// `side_t` is 24 bytes (textureoffset + rowoffset + 3 short textures + 2 byte
/// implicit padding + 8-byte sector pointer).
#[test]
fn side_t_size() {
    // textureoffset(4) + rowoffset(4) + toptexture(2) + bottomtexture(2)
    //   + midtexture(2) + implicit pad(2) + sector*(8) = 24
    assert_eq!(size_of::<side_t>(), 24);
}

/// `side_t` field offsets matching the r_defs.h layout.
#[test]
fn side_t_offsets() {
    assert_eq!(offset_of!(side_t, textureoffset), 0);
    assert_eq!(offset_of!(side_t, rowoffset), 4);
    assert_eq!(offset_of!(side_t, toptexture), 8);
    assert_eq!(offset_of!(side_t, bottomtexture), 10);
    assert_eq!(offset_of!(side_t, midtexture), 12);
    assert_eq!(offset_of!(side_t, sector), 16);
}

// ---------------------------------------------------------------------------
// seg_t  (r_defs.h — used by r_segs.c, r_bsp.c)
// ---------------------------------------------------------------------------

/// `seg_t` is 56 bytes on LP64 (two vertex pointers, offset/angle, four
/// sidedef/linedef/sector pointers).
#[test]
fn seg_t_size() {
    // v1*(8) + v2*(8) + offset(4) + angle(4) + sidedef*(8) + linedef*(8)
    //   + frontsector*(8) + backsector*(8) = 56
    assert_eq!(size_of::<seg_t>(), 56);
}

/// `seg_t` field offsets matching the r_defs.h layout.
#[test]
fn seg_t_offsets() {
    assert_eq!(offset_of!(seg_t, v1), 0);
    assert_eq!(offset_of!(seg_t, v2), 8);
    assert_eq!(offset_of!(seg_t, offset), 16);
    assert_eq!(offset_of!(seg_t, angle), 20);
    assert_eq!(offset_of!(seg_t, sidedef), 24);
    assert_eq!(offset_of!(seg_t, linedef), 32);
    assert_eq!(offset_of!(seg_t, frontsector), 40);
    assert_eq!(offset_of!(seg_t, backsector), 48);
}

// ---------------------------------------------------------------------------
// node_t  (r_defs.h — used by r_bsp.c)
// ---------------------------------------------------------------------------

/// `node_t` is 52 bytes (split origin/delta + bbox[2][4] + children[2]).
/// Maximum alignment is `int` (4 bytes), so there is no trailing padding.
#[test]
fn node_t_size() {
    // x(4)+y(4)+dx(4)+dy(4)=16 + bbox[2][4](32) + children[2](4) = 52
    // max alignment = int (4); 52 % 4 == 0 → no trailing padding
    assert_eq!(size_of::<node_t>(), 52);
}

/// `node_t` field offsets matching r_defs.h.
#[test]
fn node_t_offsets() {
    assert_eq!(offset_of!(node_t, x), 0);
    assert_eq!(offset_of!(node_t, y), 4);
    assert_eq!(offset_of!(node_t, dx), 8);
    assert_eq!(offset_of!(node_t, dy), 12);
    assert_eq!(offset_of!(node_t, bbox), 16);
    assert_eq!(offset_of!(node_t, children), 48);
}

// ---------------------------------------------------------------------------
// drawseg_t  (r_defs.h — used by r_segs.c, r_plane.c, r_things.c)
// ---------------------------------------------------------------------------

/// `drawseg_t` is 64 bytes (curline pointer + 8 ints + 3 pointers).
#[test]
fn drawseg_t_size() {
    // curline*(8) + 8×int(32) + 3×pointer(24) = 64
    // x1..tsilheight are 8 consecutive c_int fields (4 bytes each = 32 bytes),
    // followed by 3 pointers at 40, 48, 56 (8 bytes each on 64-bit).
    assert_eq!(size_of::<drawseg_t>(), 64);
}

/// `drawseg_t` field offsets matching r_defs.h.
#[test]
fn drawseg_t_offsets() {
    assert_eq!(offset_of!(drawseg_t, curline), 0);
    assert_eq!(offset_of!(drawseg_t, x1), 8);
    assert_eq!(offset_of!(drawseg_t, x2), 12);
    assert_eq!(offset_of!(drawseg_t, scale1), 16);
    assert_eq!(offset_of!(drawseg_t, scale2), 20);
    assert_eq!(offset_of!(drawseg_t, scalestep), 24);
    assert_eq!(offset_of!(drawseg_t, silhouette), 28);
    assert_eq!(offset_of!(drawseg_t, bsilheight), 32);
    assert_eq!(offset_of!(drawseg_t, tsilheight), 36);
    assert_eq!(offset_of!(drawseg_t, sprtopclip), 40);
    assert_eq!(offset_of!(drawseg_t, sprbottomclip), 48);
    assert_eq!(offset_of!(drawseg_t, maskedtexturecol), 56);
}

// ---------------------------------------------------------------------------
// vissprite_t  (r_defs.h — used by r_things.c)
// ---------------------------------------------------------------------------

/// `vissprite_t` is 80 bytes: linked-list pointers + 9 ints + alignment pad +
/// colormap pointer + mobjflags + trailing pad.
#[test]
fn vissprite_t_size() {
    // prev*(8) + next*(8) = 16
    // x1(4) + x2(4) = 8
    // gx+gy+gz+gzt+startfrac+scale+xiscale+texturemid+patch = 9×4 = 36
    // implicit pad(4) to align colormap* to 8
    // colormap*(8)
    // mobjflags(4) + trailing pad(4) to align struct to 8
    // Total = 16 + 8 + 36 + 4 + 8 + 4 + 4 = 80
    assert_eq!(size_of::<vissprite_t>(), 80);
}

/// `vissprite_t` field offsets matching r_defs.h.
#[test]
fn vissprite_t_offsets() {
    assert_eq!(offset_of!(vissprite_t, prev), 0);
    assert_eq!(offset_of!(vissprite_t, next), 8);
    assert_eq!(offset_of!(vissprite_t, x1), 16);
    assert_eq!(offset_of!(vissprite_t, x2), 20);
    assert_eq!(offset_of!(vissprite_t, gx), 24);
    assert_eq!(offset_of!(vissprite_t, gy), 28);
    assert_eq!(offset_of!(vissprite_t, gz), 32);
    assert_eq!(offset_of!(vissprite_t, gzt), 36);
    assert_eq!(offset_of!(vissprite_t, startfrac), 40);
    assert_eq!(offset_of!(vissprite_t, scale), 44);
    assert_eq!(offset_of!(vissprite_t, xiscale), 48);
    assert_eq!(offset_of!(vissprite_t, texturemid), 52);
    assert_eq!(offset_of!(vissprite_t, patch), 56);
    assert_eq!(offset_of!(vissprite_t, colormap), 64);
    assert_eq!(offset_of!(vissprite_t, mobjflags), 72);
}
