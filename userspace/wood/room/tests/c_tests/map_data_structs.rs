//! Verify the map-lump constants from `doomdata.h`.
//!
//! The constants and the on-disk record layouts live in
//! `room::engine::wad::format`; the layouts are pinned there by compile-time
//! size assertions next to their single definitions.

use room::engine::wad::format;

// ---------------------------------------------------------------------------
// MapLump constants (doomdata.h enum values must be consecutive)
// ---------------------------------------------------------------------------

/// `MapLump` enum values must be consecutive starting at 0; the map-loader
/// indexes lumps by `firstmaplump + ML_*`, so any gap or reorder would read
/// the wrong WAD lump.
#[test]
fn ml_lump_order_values() {
    assert_eq!(format::MapLump::LABEL, 0);
    assert_eq!(format::MapLump::THINGS, 1);
    assert_eq!(format::MapLump::LINEDEFS, 2);
    assert_eq!(format::MapLump::SIDEDEFS, 3);
    assert_eq!(format::MapLump::VERTEXES, 4);
    assert_eq!(format::MapLump::SEGS, 5);
    assert_eq!(format::MapLump::SSECTORS, 6);
    assert_eq!(format::MapLump::NODES, 7);
    assert_eq!(format::MapLump::SECTORS, 8);
    assert_eq!(format::MapLump::REJECT, 9);
    assert_eq!(format::MapLump::BLOCKMAP, 10);
    // Lump indices must be consecutive: BLOCKMAP is the last, at index 10.
    assert_eq!(format::MapLump::BLOCKMAP - format::MapLump::LABEL, 10);
}

// ---------------------------------------------------------------------------
// LinedefFlag bits (ML_* defines from doomdata.h)
// Each flag occupies exactly one bit and the values must be powers of two.
// ---------------------------------------------------------------------------

/// Each `LinedefFlag` (`ML_*` define from doomdata.h) must be a unique power
/// of two; the test confirms no two flags share a bit and the nine flags
/// occupy bits 0–8.  Absolute values are also checked against the C header.
#[test]
fn ml_linedef_flags() {
    // Verify every flag is a distinct power of two — no two flags may overlap.
    let all_flags = [
        format::LinedefFlag::BLOCKING,
        format::LinedefFlag::BLOCKMONSTERS,
        format::LinedefFlag::TWOSIDED,
        format::LinedefFlag::DONTPEGTOP,
        format::LinedefFlag::DONTPEGBOTTOM,
        format::LinedefFlag::SECRET,
        format::LinedefFlag::SOUNDBLOCK,
        format::LinedefFlag::DONTDRAW,
        format::LinedefFlag::MAPPED,
    ];

    // Each value must be a power of two.
    for f in all_flags {
        assert_eq!(f & (f - 1), 0, "flag {f:#x} is not a power of two");
    }

    // No two flags may share a bit.
    let mut combined: u16 = 0;
    for f in all_flags {
        assert_eq!(
            combined & f,
            0,
            "flag {f:#x} overlaps with already-seen flags"
        );
        combined |= f;
    }

    // All nine flags occupy a contiguous set of bits 0–8.
    assert_eq!(combined, (1u16 << 9) - 1);

    // Verify expected absolute values match the C header definitions.
    assert_eq!(format::LinedefFlag::BLOCKING, 1);
    assert_eq!(format::LinedefFlag::BLOCKMONSTERS, 2);
    assert_eq!(format::LinedefFlag::TWOSIDED, 4);
    assert_eq!(format::LinedefFlag::DONTPEGTOP, 8);
    assert_eq!(format::LinedefFlag::DONTPEGBOTTOM, 16);
    assert_eq!(format::LinedefFlag::SECRET, 32);
    assert_eq!(format::LinedefFlag::SOUNDBLOCK, 64);
    assert_eq!(format::LinedefFlag::DONTDRAW, 128);
    assert_eq!(format::LinedefFlag::MAPPED, 256);
}
