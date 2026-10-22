//! Verify every critical struct field offset with `offset_of!`.
//!
//! These tests use the `#[repr(C)]` engine structs.  If a field
//! offset does not match the C compiler output, the test fails,
//! catching data-type-size and alignment mismatches immediately.
//!
//! Coverage: `divline_t`, `mobj_t`, `vissprite_t`.

#![allow(non_snake_case)]

use std::mem::{offset_of, size_of};

use room::engine::{
    play::{maputl::divline_t, mobj::mobj_t},
    render_3d::things::vissprite_t,
};

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
    assert_eq!(offset_of!(mobj_t, thinker), 0);
    assert_eq!(offset_of!(mobj_t, x), 24);
    assert_eq!(offset_of!(mobj_t, y), 28);
    assert_eq!(offset_of!(mobj_t, z), 32);
    assert_eq!(offset_of!(mobj_t, angle), 56);
    assert_eq!(offset_of!(mobj_t, radius), 104);
    assert_eq!(offset_of!(mobj_t, height), 108);
    assert_eq!(offset_of!(mobj_t, kind), 128);
    assert_eq!(offset_of!(mobj_t, state), 152);
    assert_eq!(offset_of!(mobj_t, flags), 160);
    assert_eq!(offset_of!(mobj_t, health), 164);
    assert_eq!(offset_of!(mobj_t, reactiontime), 184);
    assert_eq!(offset_of!(mobj_t, player), 192);
    assert_eq!(offset_of!(mobj_t, tracer), 216);
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
