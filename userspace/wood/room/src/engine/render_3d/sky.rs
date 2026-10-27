//! Rust port of `vendor/doomgeneric/r_sky.c`.
//!
//! Sky-rendering state and one trivial init function.  The sky is drawn as
//! a wall-like texture that wraps around the full 360-degree view.  A 1024-
//! column sky map equals one full revolution; the default Doom sky is 256
//! columns and repeats four times on a 320-pixel-wide screen.
//!
//! `skytexturemid` is the vertical centre used by `r_plane.c` when drawing
//! sky columns.  Horizontal mapping uses `ANGLETOSKYSHIFT` (22 bits, defined
//! in `r_sky.h`) to convert a BAM angle into a column index.

#![allow(non_upper_case_globals, non_snake_case)]

use crate::engine::math::fixed::FRACUNIT;

/// Sky state (C `r_sky.c` globals), the renderer's `sky`.
pub struct SkyState {
    /// WAD lump index of the sky flat (`F_SKY1`), filled in by `g_game.c`
    /// via `R_FlatNumForName(SKYFLATNAME)`.
    ///
    /// Sectors whose ceiling flat equals this value are treated as open sky by
    /// the BSP, seg, and plane renderers (`r_bsp.c`, `r_segs.c`, `r_plane.c`,
    /// `p_map.c`, `p_mobj.c`).
    pub skyflatnum: i32,
    /// Texture number of the current sky texture (SKY1 / SKY2 / SKY3 / SKY4),
    /// assigned by `g_game.c` at level start.
    ///
    /// `r_plane.c` uses this index when calling `R_GetColumn` to fetch sky
    /// columns, and `r_data.c` marks it as always-present during precaching.
    pub skytexture: i32,
    /// Vertical midpoint for sky column drawing, in 16.16 fixed-point units.
    ///
    /// Reset to `100 * FRACUNIT` every time the view size changes (see
    /// `R_InitSkyMap`).  Used by `r_plane.c` as `dc_texturemid` when rendering
    /// sky spans.
    pub skytexturemid: i32,
}

impl SkyState {
    /// The state before `R_InitSkyMap` and level setup: all zero.
    pub const fn new() -> Self {
        Self {
            skyflatnum: 0,
            skytexture: 0,
            skytexturemid: 0,
        }
    }
}

impl Default for SkyState {
    fn default() -> Self {
        Self::new()
    }
}

/// Reset sky-mapping state whenever the view size changes.
///
/// Sets `skytexturemid` to `100 * FRACUNIT` (6553600), the fixed vertical
/// centre for sky column drawing.  The `skyflatnum` assignment that appears
/// commented-out in the C source is performed elsewhere (in `g_game.c`).
///
/// Called by `r_main.c` (`R_Init` -> `R_ExecuteSetViewSize`) whenever the
/// view size is reconfigured.
pub fn R_InitSkyMap(sky: &mut SkyState) {
    // skyflatnum = R_FlatNumForName(SKYFLATNAME);   // done elsewhere
    sky.skytexturemid = 100 * FRACUNIT;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_sets_texturemid() {
        let mut sky = SkyState::new();
        R_InitSkyMap(&mut sky);
        assert_eq!(sky.skytexturemid, 100 * 65536);
    }

    #[test]
    fn state_defaults_to_zero() {
        let sky = SkyState::new();
        assert_eq!(
            (sky.skyflatnum, sky.skytexture, sky.skytexturemid),
            (0, 0, 0)
        );
    }
}
