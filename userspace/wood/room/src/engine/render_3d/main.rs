//! Renderer main loop, view setup, and BSP/geometry/trigonometry utilities.
//!
//! Rust port of `vendor/doomgeneric/r_main.c`. Hosts the renderer view state
//! ([`ViewState`] in [`VIEW`]: viewport dimensions, view position/angle,
//! light tables, column/span function pointers), the engine-wide
//! [`VALIDCOUNT`], and the per-frame entry point [`R_RenderPlayerView`].

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::ptr;

use crate::{
    engine::{
        doom::player::PlayerT,
        global::Global,
        interface::video::{SCREENHEIGHT as SCREENHEIGHT_IV, SCREENWIDTH as SCREENWIDTH_IV},
        map::{SegId, SubsectorId, node_t},
        math::{
            self,
            bbox::BBox,
            fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, angle_t, fixed_t},
            tables::{ANG90, ANG180, ANG270, ANGLETOFINESHIFT, SlopeDiv},
        },
    },
    types::Boolean,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of fine-angle steps spanning the horizontal field of view (90 degrees
/// expressed in fine-angle units; `FINEANGLES / 4 = 2048`).
const FIELDOFVIEW: i32 = 2048;

/// Screen pixel width, mirrored from `i_video` as a `usize` for array sizing.
const SCREENWIDTH: usize = SCREENWIDTH_IV as usize;

/// Screen pixel height, mirrored from `i_video` as a `usize` for array sizing.
const SCREENHEIGHT: usize = SCREENHEIGHT_IV as usize;

/// Number of distinct light levels used in the `scalelight` / `zlight` tables.
const LIGHTLEVELS: usize = 16;

/// Shift used to convert a wall scale value into a light-level index (unused in
/// this module but exported for sibling renderer modules).
#[allow(dead_code)]
const LIGHTSEGSHIFT: u32 = 4;

/// Maximum number of scale steps in the `scalelight` table (one entry per
/// screen-pixel column-scale bucket).
const MAXLIGHTSCALE: usize = 48;

/// Shift applied to a column scale value before indexing `scalelight`.
const LIGHTSCALESHIFT: u32 = 12;

/// Maximum number of Z-distance steps in the `zlight` table.
const MAXLIGHTZ: usize = 128;

/// Shift applied to a Z-distance value before indexing `zlight`.
const LIGHTZSHIFT: u32 = 20;

/// Number of colormap entries (palette remapping tables); each is 256 bytes.
const NUMCOLORMAPS: usize = 32;

/// Divisor applied to the raw scale/distance value when mapping to a light
/// level, controlling how quickly lighting falls off with distance.
const DISTMAP: usize = 2;

/// `FRACBITS - SLOPEBITS = 16 - 11 = 5`.
///
/// Used to right-shift a fixed-point fraction before indexing `tantoangle[]`
/// in [`R_PointToDist`]. Matches `DBITS` in `vendor/doomgeneric/tables.h`.
// DBITS = FRACBITS - SLOPEBITS = 16 - 11 = 5 (matches vendor/doomgeneric/tables.h).
const DBITS: u32 = 5;

/// Raw colormap byte type; a palette index remap table entry.
type lighttable_t = u8;

/// Number of linedefs processed in the current frame (profiling counter).
///
/// Declared by `r_main.c` but never updated by the renderer.
pub const linecount: i32 = 0;

// ---------------------------------------------------------------------------
// State defined by this module
// ---------------------------------------------------------------------------

/// Renderer view state (C `r_main.c` globals).
pub struct ViewState {
    /// Additive angle offset applied to the player's map angle before rendering,
    /// used by the automap and demo-playback angle overrides. Zero during normal
    /// gameplay.
    pub viewangleoffset: i32,
    /// Active fixed colormap pointer, or null when no override is in effect.
    ///
    /// Set to a non-null colormap when the player has an invulnerability or
    /// light-amp powerup active (`player.fixedcolormap != 0`). When non-null,
    /// all walls, floors, and sprites use this single colormap instead of the
    /// distance-based `scalelight`/`zlight` entries.
    pub fixedcolormap: *mut lighttable_t,
    /// X coordinate of the viewport centre in screen pixels.
    ///
    /// Recomputed by [`R_ExecuteSetViewSize`] whenever the view size changes.
    pub centerx: i32,
    /// Y coordinate of the viewport centre in screen pixels.
    ///
    /// Recomputed by [`R_ExecuteSetViewSize`] whenever the view size changes.
    pub centery: i32,
    /// `centerx` expressed as a 16.16 fixed-point value (`centerx << FRACBITS`).
    ///
    /// Used by [`R_InitTextureMapping`] and projection arithmetic in
    /// [`R_ExecuteSetViewSize`].
    pub centerxfrac: fixed_t,
    /// `centery` expressed as a 16.16 fixed-point value (`centery << FRACBITS`).
    ///
    /// Precomputed by [`R_ExecuteSetViewSize`]; used by wall and sprite renderers
    /// in `r_segs` and `r_things` to project top/bottom screen coordinates.
    pub centeryfrac: fixed_t,
    /// Fixed-point focal length (horizontal projection constant).
    ///
    /// Equal to `centerxfrac`; used by [`R_ScaleFromGlobalAngle`] and the
    /// texture mapping setup in [`R_InitTextureMapping`]. Represents the
    /// distance from the eye to the projection plane in fixed-point units.
    pub projection: fixed_t,
    /// Number of frames rendered since [`R_Init`] was called.
    ///
    /// Incremented once per frame in [`R_SetupFrame`]. Used for profiling.
    pub framecount: i32,
    /// Number of subsectors drawn in the current frame.
    ///
    /// Reset to zero in [`R_SetupFrame`]; incremented in `r_bsp`. Used for
    /// profiling.
    pub sscount: i32,
    /// View position X in map fixed-point units. Set each frame by
    /// [`R_SetupFrame`] from the player mobj's `x` field.
    pub viewx: fixed_t,
    /// View position Y in map fixed-point units. Set each frame by
    /// [`R_SetupFrame`] from the player mobj's `y` field.
    pub viewy: fixed_t,
    /// View height Z in map fixed-point units (player eye height). Set each
    /// frame by [`R_SetupFrame`] from `player.viewz`.
    pub viewz: fixed_t,
    /// Current view angle as a Binary Angle Measurement (BAM) `u32`.
    ///
    /// Set each frame in [`R_SetupFrame`] from the player mobj angle plus
    /// `viewangleoffset`. Used throughout the BSP traversal and texture
    /// mapping pipeline.
    pub viewangle: angle_t,
    /// `cos(viewangle)` in 16.16 fixed-point. Precomputed each frame in
    /// [`R_SetupFrame`] for fast world-space projection.
    pub viewcos: fixed_t,
    /// `sin(viewangle)` in 16.16 fixed-point. Precomputed each frame in
    /// [`R_SetupFrame`] for fast world-space projection.
    pub viewsin: fixed_t,
    /// Pointer to the player struct whose view is currently being rendered.
    ///
    /// Set at the start of each frame by [`R_SetupFrame`]; read by several
    /// renderer subsystems that need player-specific state (e.g. weapon sprites).
    pub viewplayer: *mut PlayerT,
    /// Detail level shift: `0` = high detail, `1` = low detail (half-width
    /// columns doubled horizontally). Controls which column/span draw functions
    /// are active and affects several scaling calculations.
    pub detailshift: i32,
    /// Half the horizontal field of view as a BAM angle.
    ///
    /// Set by [`R_InitTextureMapping`] to `xtoviewangle[0]` - the largest view
    /// angle that still maps to screen column 0. Used by the BSP clipper in
    /// `r_bsp` to cull out-of-frustum segs.
    pub clipangle: angle_t,
    /// Maps fine-angle index to screen X column.
    ///
    /// `viewangletox[i]` is the screen column (or sentinel `-1` /
    /// `viewwidth+1` for out-of-frustum angles) for fine-angle `i`. Indexed
    /// by `(viewangle >> ANGLETOFINESHIFT)`. Sized one element larger than
    /// strictly necessary (`FINEANGLES/2 + 1`) to match `r_bsp`'s defensive
    /// bounds.
    // r_bsp.rs originally declared this as [i32; FINEANGLES/2 + 1] to guard
    // against a potential off-by-one in the original C code. Keep the same
    // size so the two modules agree.
    pub viewangletox: [i32; math::tables::FINEANGLES / 2 + 1],
    /// Maps screen X column to the smallest view angle that projects onto that
    /// column.
    ///
    /// `xtoviewangle[x]` gives the left-edge angle of the frustum slice at
    /// column `x`. Sized `SCREENWIDTH + 1` to include the right-edge sentinel.
    pub xtoviewangle: [angle_t; SCREENWIDTH + 1],
    /// Distance-to-light lookup table indexed by `[light_level][scale]`.
    ///
    /// `scalelight[i][j]` points into the master `colormaps` array. `i` is
    /// derived from the sector light level, `j` from the projected wall/sprite
    /// scale. Recomputed by [`R_ExecuteSetViewSize`] because it depends on
    /// `viewwidth`. Rows are addressed through [`scalelight_row`].
    pub scalelight: [[*mut lighttable_t; MAXLIGHTSCALE]; LIGHTLEVELS],
    /// Fixed-scale colormap array used when `fixedcolormap` is active.
    ///
    /// All `MAXLIGHTSCALE` entries are set to `fixedcolormap` in
    /// [`R_SetupFrame`], so that wall-light lookup code does not need a special
    /// case for fixed-colormap mode.
    pub scalelightfixed: [*mut lighttable_t; MAXLIGHTSCALE],
    /// Z-distance-to-light lookup table indexed by `[light_level][z_bucket]`.
    ///
    /// `zlight[i][j]` points into `colormaps`. Used for flat (floor/ceiling)
    /// and sprite lighting. Computed once in [`R_InitLightTables`] (unlike
    /// `scalelight`, it does not depend on `viewwidth`). Rows are addressed
    /// through [`zlight_row`].
    pub zlight: [[*mut lighttable_t; MAXLIGHTZ]; LIGHTLEVELS],
    /// Additional light bonus added to the sector's base light level, produced
    /// by muzzle flashes and similar effects. Set each frame from
    /// `player.extralight` in [`R_SetupFrame`].
    pub extralight: i32,
    /// Column-drawing function for solid walls and sprites.
    ///
    /// The normal or low-detail column renderer (`R_DrawColumn` or
    /// `R_DrawColumnLow`). The C `colfunc` is temporarily switched to the
    /// fuzz or translated variant per sprite and then reset to `basecolfunc`;
    /// `R_DrawVisSprite` instead picks the variant locally, so this always
    /// holds the base function.
    pub colfunc: ColumnFunction,
    /// Fuzz (partial-invisibility) column-drawing function.
    ///
    /// `R_DrawFuzzColumn` or `R_DrawFuzzColumnLow` depending on the active
    /// detail level.
    pub fuzzcolfunc: ColumnFunction,
    /// Translated (palette-remapped) column-drawing function.
    ///
    /// `R_DrawTranslatedColumn` or `R_DrawTranslatedColumnLow`. Used for
    /// colored player sprites in multiplayer.
    pub transcolfunc: ColumnFunction,
    /// Horizontal span (floor/ceiling) drawing function.
    ///
    /// `R_DrawSpan` or `R_DrawSpanLow` depending on the active detail level.
    pub spanfunc: SpanFunction,
    /// Flag set by [`R_SetViewSize`] when a view-size change is pending.
    ///
    /// [`R_ExecuteSetViewSize`] checks this at the start of each frame and
    /// applies the pending change if set. The deferred approach avoids changing
    /// viewport dimensions mid-frame.
    pub setsizeneeded: Boolean,
    /// Pending viewport block size (1-11). Set by [`R_SetViewSize`] and
    /// consumed by [`R_ExecuteSetViewSize`]. Value 11 selects full-screen
    /// rendering.
    pub setblocks: i32,
    /// Pending detail level (0 = high, 1 = low). Set by [`R_SetViewSize`] and
    /// consumed by [`R_ExecuteSetViewSize`].
    pub setdetail: i32,
}

impl ViewState {
    /// The state before `R_Init`: zero view, empty tables, high-detail drawers.
    pub const fn new() -> Self {
        Self {
            viewangleoffset: 0,
            fixedcolormap: ptr::null_mut(),
            centerx: 0,
            centery: 0,
            centerxfrac: 0,
            centeryfrac: 0,
            projection: 0,
            framecount: 0,
            sscount: 0,
            viewx: 0,
            viewy: 0,
            viewz: 0,
            viewangle: 0,
            viewcos: 0,
            viewsin: 0,
            viewplayer: ptr::null_mut(),
            detailshift: 0,
            clipangle: 0,
            viewangletox: [0; math::tables::FINEANGLES / 2 + 1],
            xtoviewangle: [0; SCREENWIDTH + 1],
            scalelight: [[ptr::null_mut(); MAXLIGHTSCALE]; LIGHTLEVELS],
            scalelightfixed: [ptr::null_mut(); MAXLIGHTSCALE],
            zlight: [[ptr::null_mut(); MAXLIGHTZ]; LIGHTLEVELS],
            extralight: 0,
            colfunc: R_DrawColumn,
            fuzzcolfunc: R_DrawFuzzColumn,
            transcolfunc: R_DrawTranslatedColumn,
            spanfunc: R_DrawSpan,
            setsizeneeded: Boolean::FALSE,
            setblocks: 0,
            setdetail: 0,
        }
    }
}

impl Default for ViewState {
    fn default() -> Self {
        Self::new()
    }
}

/// The renderer view state.
pub static VIEW: Global<ViewState> = Global::new(ViewState::new());

/// Monotonically incrementing counter; bumped once per frame in
/// [`R_SetupFrame`] and by the play code before each map search. Sectors
/// and linedefs tag themselves with `validcount` when first visited so they
/// are not processed twice.
pub static VALIDCOUNT: Global<i32> = Global::new(1);

/// Stable address of `scalelight[level]`, for the `walllights` and
/// `spritelights` row pointers that outlive any borrow of [`VIEW`].
pub fn scalelight_row(level: usize) -> *const *mut lighttable_t {
    // Addresses the static's storage directly, so the row stays valid unguarded.
    unsafe { (&raw const (*VIEW.as_ptr()).scalelight[level]).cast() }
}

/// Stable address of `scalelightfixed`, the `walllights` row while a fixed
/// colormap is active.
pub fn scalelightfixed_row() -> *const *mut lighttable_t {
    // Addresses the static's storage directly, so the row stays valid unguarded.
    unsafe { (&raw const (*VIEW.as_ptr()).scalelightfixed).cast() }
}

/// Stable address of `zlight[level]`, for the plane renderer's
/// `planezlight` row pointer.
pub fn zlight_row(level: usize) -> *const *mut lighttable_t {
    // Addresses the static's storage directly, so the row stays valid unguarded.
    unsafe { (&raw const (*VIEW.as_ptr()).zlight[level]).cast() }
}

// ---------------------------------------------------------------------------
// Imports from other modules
// ---------------------------------------------------------------------------

use crate::engine::{
    doom::main_loop::NetUpdate,
    misc::menu::MENU,
    play::setup::{LEVEL, LevelState},
    render_3d::{
        bsp::{R_ClearClipSegs, R_ClearDrawSegs, R_RenderBSPNode},
        data::{R_InitData, RENDER_DATA},
        draw::{
            ColumnFunction, DRAW, R_DrawColumn, R_DrawColumnLow, R_DrawFuzzColumn,
            R_DrawFuzzColumnLow, R_DrawSpan, R_DrawSpanLow, R_DrawTranslatedColumn,
            R_DrawTranslatedColumnLow, R_InitBuffer, R_InitTranslationTables, SpanFunction,
        },
        plane::{PLANES, R_ClearPlanes, R_DrawPlanes, R_InitPlanes},
        segs::{WALL, WallState},
        things::{R_ClearSprites, R_DrawMasked, SPRITES},
    },
};

// ---------------------------------------------------------------------------
// R_AddPointToBox
// ---------------------------------------------------------------------------

/// Expand a bounding box so that it encloses the given map-coordinate point.
///
/// Equivalent to `R_AddPointToBox` in `r_main.c`. Modifies the four
/// fixed-point values at `box_` in the [`BBox`] layout (`LEFT`, `RIGHT`,
/// `BOTTOM`, `TOP`).
///
/// # Safety
/// `box_` must point to a valid, writable array of at least four `fixed_t`
/// values laid out in [`BBox`] index order.
pub unsafe fn R_AddPointToBox(x: i32, y: i32, box_: *mut fixed_t) {
    if x < *box_.add(BBox::LEFT) {
        *box_.add(BBox::LEFT) = x;
    }
    if x > *box_.add(BBox::RIGHT) {
        *box_.add(BBox::RIGHT) = x;
    }
    if y < *box_.add(BBox::BOTTOM) {
        *box_.add(BBox::BOTTOM) = y;
    }
    if y > *box_.add(BBox::TOP) {
        *box_.add(BBox::TOP) = y;
    }
}

// ---------------------------------------------------------------------------
// R_PointOnSide
// ---------------------------------------------------------------------------

/// Determine which side of a BSP partition plane a map point lies on.
///
/// Returns `0` for the front (right) side and `1` for the back (left) side.
/// Uses fast sign-bit shortcuts for axis-aligned partitions before falling
/// back to a full cross-product test.
///
/// Equivalent to `R_PointOnSide` in `r_main.c`.
pub fn R_PointOnSide(x: fixed_t, y: fixed_t, node: &node_t) -> i32 {
    if node.dx == 0 {
        if x <= node.x {
            return (node.dy > 0) as i32;
        }
        return (node.dy < 0) as i32;
    }
    if node.dy == 0 {
        if y <= node.y {
            return (node.dx < 0) as i32;
        }
        return (node.dx > 0) as i32;
    }

    let dx = x - node.x;
    let dy = y - node.y;

    // Try to quickly decide by looking at sign bits.
    if ((node.dy ^ node.dx ^ dx ^ dy) as u32) & 0x80000000 != 0 {
        if ((node.dy ^ dx) as u32) & 0x80000000 != 0 {
            return 1;
        }
        return 0;
    }

    let left = FixedMul(node.dy >> FRACBITS, dx);
    let right = FixedMul(dy, node.dx >> FRACBITS);

    if right < left { 0 } else { 1 }
}

// ---------------------------------------------------------------------------
// R_PointOnSegSide
// ---------------------------------------------------------------------------

/// Determine which side of a seg (map line segment) a point lies on.
///
/// Returns `0` for the front side and `1` for the back side, using the same
/// sign-bit shortcut as [`R_PointOnSide`].
///
/// Equivalent to `R_PointOnSegSide` in `r_main.c`.
pub fn R_PointOnSegSide(level: &LevelState, x: fixed_t, y: fixed_t, line: SegId) -> i32 {
    let line = &level.segs[line];
    let (v1, v2) = (level.vertexes[line.v1], level.vertexes[line.v2]);
    let lx = v1.x;
    let ly = v1.y;

    let ldx = v2.x - lx;
    let ldy = v2.y - ly;

    if ldx == 0 {
        if x <= lx {
            return (ldy > 0) as i32;
        }
        return (ldy < 0) as i32;
    }
    if ldy == 0 {
        if y <= ly {
            return (ldx < 0) as i32;
        }
        return (ldx > 0) as i32;
    }

    let dx = x - lx;
    let dy = y - ly;

    // Try to quickly decide by looking at sign bits.
    if ((ldy ^ ldx ^ dx ^ dy) as u32) & 0x80000000 != 0 {
        if ((ldy ^ dx) as u32) & 0x80000000 != 0 {
            return 1;
        }
        return 0;
    }

    let left = FixedMul(ldy >> FRACBITS, dx);
    let right = FixedMul(dy, ldx >> FRACBITS);

    if right < left { 0 } else { 1 }
}

// ---------------------------------------------------------------------------
// R_PointToAngle
// ---------------------------------------------------------------------------

/// Classify a relative vector `(dx, dy)` into one of eight octants and look
/// up the corresponding BAM angle from the `tantoangle` table.
///
/// This is the pure octant-classification core shared by [`R_PointToAngle`]
/// and [`R_PointToAngle2`]; it never reads or writes any global state.
/// Returns 0 for the zero vector.
fn point_to_angle_from_delta(mut dx: fixed_t, mut dy: fixed_t) -> angle_t {
    if dx == 0 && dy == 0 {
        return 0;
    }

    if dx >= 0 {
        // dx >= 0
        if dy >= 0 {
            // dy >= 0
            if dx > dy {
                // octant 0
                math::tables::tantoangle[SlopeDiv(dy as u32, dx as u32) as usize]
            } else {
                // octant 1
                ANG90 - 1 - math::tables::tantoangle[SlopeDiv(dx as u32, dy as u32) as usize]
            }
        } else {
            // dy < 0
            dy = -dy;
            if dx > dy {
                // octant 8
                0u32.wrapping_sub(math::tables::tantoangle[SlopeDiv(dy as u32, dx as u32) as usize])
            } else {
                // octant 7
                ANG270 + math::tables::tantoangle[SlopeDiv(dx as u32, dy as u32) as usize]
            }
        }
    } else {
        // dx < 0
        dx = -dx;
        if dy >= 0 {
            // dy >= 0
            if dx > dy {
                // octant 3
                ANG180 - 1 - math::tables::tantoangle[SlopeDiv(dy as u32, dx as u32) as usize]
            } else {
                // octant 2
                ANG90 + math::tables::tantoangle[SlopeDiv(dx as u32, dy as u32) as usize]
            }
        } else {
            // dy < 0
            dy = -dy;
            if dx > dy {
                // octant 4
                ANG180 + math::tables::tantoangle[SlopeDiv(dy as u32, dx as u32) as usize]
            } else {
                // octant 5
                ANG270 - 1 - math::tables::tantoangle[SlopeDiv(dx as u32, dy as u32) as usize]
            }
        }
    }
}

/// Convert an absolute map coordinate to a view angle (BAM `u32`).
///
/// Subtracts the current `viewx`/`viewy` to get a relative vector, then
/// classifies the vector into one of eight octants and looks up the angle
/// using the `tantoangle` table. Returns 0 for the view position itself.
///
/// Equivalent to `R_PointToAngle` in `r_main.c`.
///
/// Reads `viewx` and `viewy` from [`VIEW`], so the result is only
/// meaningful once [`R_SetupFrame`] has set them.
pub fn R_PointToAngle(x: fixed_t, y: fixed_t) -> angle_t {
    let (viewx, viewy) = {
        let view = VIEW.borrow();
        (view.viewx, view.viewy)
    };
    point_to_angle_from_delta(x - viewx, y - viewy)
}

// ---------------------------------------------------------------------------
// R_PointToAngle2
// ---------------------------------------------------------------------------

/// Compute the BAM angle from map point `(x1, y1)` to map point `(x2, y2)`.
///
/// Computes the delta `(x2 - x1, y2 - y1)` and classifies it directly via the
/// shared octant lookup. Unlike the C original (and earlier Rust port), this
/// implementation does not touch the `viewx`/`viewy` view state, so it is
/// reentrant and safe to call between a read and a use of the view position.
///
/// Equivalent to `R_PointToAngle2` in `r_main.c`, with the global-aliasing
/// trick removed.
pub fn R_PointToAngle2(x1: fixed_t, y1: fixed_t, x2: fixed_t, y2: fixed_t) -> angle_t {
    point_to_angle_from_delta(x2 - x1, y2 - y1)
}

// ---------------------------------------------------------------------------
// R_PointToDist
// ---------------------------------------------------------------------------

/// Compute the distance from the current view position to a map point.
///
/// Uses the `tantoangle` and `finesine` tables to compute the Euclidean
/// distance via a cosine projection. Handles `dx == 0` to avoid division by
/// zero (matches the udm1.wad crash fix present in the C source).
///
/// Equivalent to `R_PointToDist` in `r_main.c`.
///
/// Reads `viewx` and `viewy` from [`VIEW`], so the result is only
/// meaningful once [`R_SetupFrame`] has set them.
pub fn R_PointToDist(x: fixed_t, y: fixed_t) -> fixed_t {
    let (viewx, viewy) = {
        let view = VIEW.borrow();
        (view.viewx, view.viewy)
    };
    let mut dx = (x - viewx).wrapping_abs();
    let mut dy = (y - viewy).wrapping_abs();

    if dy > dx {
        std::mem::swap(&mut dx, &mut dy);
    }

    let frac = if dx != 0 { FixedDiv(dy, dx) } else { 0 };

    let angle =
        (math::tables::tantoangle[(frac as u32 >> DBITS) as usize] + ANG90) >> ANGLETOFINESHIFT;

    FixedDiv(dx, math::tables::finesine[angle as usize])
}

// ---------------------------------------------------------------------------
// R_InitPointToAngle
// ---------------------------------------------------------------------------

/// No-op initialiser kept for ABI compatibility.
///
/// In the original Doom source, `R_InitPointToAngle` built the `tantoangle`
/// lookup table at runtime. The table is now precomputed in `tables.c` (and
/// [`math::tables`]), so this function has no work to do.
pub fn R_InitPointToAngle() {
    // UNUSED - now getting from tables.c
}

// ---------------------------------------------------------------------------
// R_ScaleFromGlobalAngle
// ---------------------------------------------------------------------------

/// Compute the texture-mapping scale for a wall column at the given view
/// angle.
///
/// Returns the fixed-point scale factor used to stretch or shrink a wall
/// texture column. Clamps the result to `[256, 64 * FRACUNIT]` to avoid
/// extreme near/far values.
///
/// `wall.rw_distance` (distance from view to the wall normal) and
/// `wall.rw_normalangle` (angle of the wall's outward normal) must be set
/// before calling this function; `r_segs` computes both per wall.
///
/// Equivalent to `R_ScaleFromGlobalAngle` in `r_main.c`.
///
/// Reads `viewangle`, `projection` and `detailshift` from `view` and the
/// `r_segs` wall state.
pub fn R_ScaleFromGlobalAngle(view: &ViewState, wall: &WallState, visangle: angle_t) -> fixed_t {
    let anglea = ANG90.wrapping_add(visangle.wrapping_sub(view.viewangle));
    let angleb = ANG90.wrapping_add(visangle.wrapping_sub(wall.rw_normalangle));

    let sinea = math::tables::finesine[(anglea >> ANGLETOFINESHIFT) as usize];
    let sineb = math::tables::finesine[(angleb >> ANGLETOFINESHIFT) as usize];
    let num = FixedMul(view.projection, sineb) << view.detailshift;
    let den = FixedMul(wall.rw_distance, sinea);

    if den > num >> 16 {
        let mut scale = FixedDiv(num, den);
        scale = scale.clamp(256, 64 * FRACUNIT);
        scale
    } else {
        64 * FRACUNIT
    }
}

// ---------------------------------------------------------------------------
// R_InitTables
// ---------------------------------------------------------------------------

/// No-op initialiser kept for ABI compatibility.
///
/// In the original source, `R_InitTables` computed `finetangent` and
/// `finesine` at runtime. Both tables are now precomputed in `tables.c`
/// (and [`math::tables`]).
pub fn R_InitTables() {
    // UNUSED: now getting from tables.c
}

// ---------------------------------------------------------------------------
// R_InitTextureMapping
// ---------------------------------------------------------------------------

/// Build the `viewangletox` and `xtoviewangle` lookup tables for the
/// current viewport geometry, then set `clipangle`.
///
/// The focal length is derived from `centerxfrac` and the `finetangent`
/// table so that `FIELDOFVIEW` fine-angle steps span exactly `viewwidth`
/// pixels. After building both tables the function removes the sentinel
/// `-1`/`viewwidth+1` values from `viewangletox` (fencepost cleanup).
///
/// Called by [`R_ExecuteSetViewSize`] whenever the viewport is resized.
///
/// # Safety
/// Reads and writes [`VIEW`] and reads `viewwidth`. Must be called after
/// `viewwidth` and `centerxfrac` have been set by [`R_ExecuteSetViewSize`].
pub unsafe fn R_InitTextureMapping() {
    let viewwidth = DRAW.borrow().viewwidth;
    let mut view = VIEW.borrow_mut();
    let view = &mut *view;

    // Use tangent table to generate viewangletox:
    //  viewangletox will give the next greatest x after the view angle.
    //
    // Calc focallength so FIELDOFVIEW angles covers SCREENWIDTH.
    let focallength = FixedDiv(
        view.centerxfrac,
        math::tables::finetangent[math::tables::FINEANGLES / 4 + FIELDOFVIEW as usize / 2],
    );

    for i in 0..math::tables::FINEANGLES / 2 {
        let t: i32;
        if math::tables::finetangent[i] > FRACUNIT * 2 {
            t = -1;
        } else if math::tables::finetangent[i] < -FRACUNIT * 2 {
            t = viewwidth + 1;
        } else {
            let mut tt = FixedMul(math::tables::finetangent[i], focallength);
            tt = (view.centerxfrac - tt + FRACUNIT - 1) >> FRACBITS;
            if tt < -1 {
                t = -1;
            } else if tt > viewwidth + 1 {
                t = viewwidth + 1;
            } else {
                t = tt;
            }
        }
        view.viewangletox[i] = t;
    }

    // Scan viewangletox[] to generate xtoviewangle[]:
    //  xtoviewangle will give the smallest view angle that maps to x.
    for x in 0..=viewwidth as usize {
        let mut i = 0usize;
        while view.viewangletox[i] > x as i32 {
            i += 1;
        }
        view.xtoviewangle[x] = ((i as u32) << ANGLETOFINESHIFT).wrapping_sub(ANG90);
    }

    // Take out the fencepost cases from viewangletox.
    for i in 0..math::tables::FINEANGLES / 2 {
        let mut t = FixedMul(math::tables::finetangent[i], focallength);
        t = view.centerx - t;

        if view.viewangletox[i] == -1 {
            view.viewangletox[i] = 0;
        } else if view.viewangletox[i] == viewwidth + 1 {
            view.viewangletox[i] = viewwidth;
        }
    }

    view.clipangle = view.xtoviewangle[0];
}

// ---------------------------------------------------------------------------
// R_InitLightTables
// ---------------------------------------------------------------------------

/// Precompute the Z-distance-based light table `zlight`.
///
/// For each combination of sector light level and Z-distance bucket, computes
/// a pointer into the master `colormaps` array. The Z-distance buckets are
/// shifted by `LIGHTZSHIFT` before lookup. Only the distance-based
/// `zlight` table is built here; the scale-based `scalelight` table
/// depends on `viewwidth` and is rebuilt in [`R_ExecuteSetViewSize`].
///
/// Equivalent to `R_InitLightTables` in `r_main.c`.
///
/// # Safety
/// Reads `colormaps` from `r_data`; that pointer must be non-null and point
/// to `NUMCOLORMAPS * 256` valid bytes. Called during startup by [`R_Init`].
pub unsafe fn R_InitLightTables() {
    let colormaps = RENDER_DATA.borrow().colormaps;
    let mut view = VIEW.borrow_mut();
    for i in 0..LIGHTLEVELS {
        let startmap = (((LIGHTLEVELS - 1 - i) * 2) * NUMCOLORMAPS / LIGHTLEVELS) as i32;
        for j in 0..MAXLIGHTZ {
            let mut scale = FixedDiv(
                (SCREENWIDTH as i32 / 2) * FRACUNIT,
                ((j + 1) << LIGHTZSHIFT) as i32,
            );
            scale >>= LIGHTSCALESHIFT;
            let mut level = startmap - scale / DISTMAP as i32;

            if level < 0 {
                level = 0;
            }
            if level >= NUMCOLORMAPS as i32 {
                level = NUMCOLORMAPS as i32 - 1;
            }

            view.zlight[i][j] = colormaps.add(level as usize * 256);
        }
    }
}

// ---------------------------------------------------------------------------
// R_SetViewSize
// ---------------------------------------------------------------------------

/// Schedule a viewport size change for the next frame.
///
/// Sets `setsizeneeded`, `setblocks`, and `setdetail` so that
/// [`R_ExecuteSetViewSize`] will apply them at the start of the next rendered
/// frame. Safe to call mid-frame because the actual resize is deferred.
///
/// Equivalent to `R_SetViewSize` in `r_main.c`.
pub fn R_SetViewSize(blocks: i32, detail: i32) {
    let mut view = VIEW.borrow_mut();
    view.setsizeneeded = Boolean::TRUE;
    view.setblocks = blocks;
    view.setdetail = detail;
}

// ---------------------------------------------------------------------------
// R_ExecuteSetViewSize
// ---------------------------------------------------------------------------

/// Apply a pending viewport size change.
///
/// Computes all viewport dimensions (`viewwidth`, `viewheight`,
/// `scaledviewwidth`, `centerx`, `centery`, `centerxfrac`, `centeryfrac`,
/// `projection`, `detailshift`), selects the appropriate column/span draw
/// function pointers, rebuilds the texture-mapping tables, and recomputes both
/// the `scalelight` and `yslope`/`distscale` plane tables.
///
/// Only called when `setsizeneeded` is `TRUE`; normally invoked once per
/// frame from the game loop before rendering begins.
///
/// Equivalent to `R_ExecuteSetViewSize` in `r_main.c`.
///
/// # Safety
/// Writes a large number of renderer state fields and calls several
/// initialisation helpers. Must not be called while a frame render is in
/// progress.
pub unsafe fn R_ExecuteSetViewSize() {
    let (scaledviewwidth, viewheight, viewwidth, detailshift) = {
        let mut view = VIEW.borrow_mut();
        let mut draw = DRAW.borrow_mut();
        view.setsizeneeded = Boolean::FALSE;

        if view.setblocks == 11 {
            draw.scaledviewwidth = SCREENWIDTH as i32;
            draw.viewheight = SCREENHEIGHT as i32;
        } else {
            draw.scaledviewwidth = view.setblocks * 32;
            draw.viewheight = (view.setblocks * 168 / 10) & !7;
        }

        view.detailshift = view.setdetail;
        draw.viewwidth = draw.scaledviewwidth >> view.detailshift;

        view.centery = draw.viewheight / 2;
        view.centerx = draw.viewwidth / 2;
        view.centerxfrac = view.centerx << FRACBITS;
        view.centeryfrac = view.centery << FRACBITS;
        view.projection = view.centerxfrac;

        if view.detailshift == 0 {
            view.colfunc = R_DrawColumn;
            view.fuzzcolfunc = R_DrawFuzzColumn;
            view.transcolfunc = R_DrawTranslatedColumn;
            view.spanfunc = R_DrawSpan;
        } else {
            view.colfunc = R_DrawColumnLow;
            view.fuzzcolfunc = R_DrawFuzzColumnLow;
            view.transcolfunc = R_DrawTranslatedColumnLow;
            view.spanfunc = R_DrawSpanLow;
        }

        (
            draw.scaledviewwidth,
            draw.viewheight,
            draw.viewwidth,
            view.detailshift,
        )
    };

    R_InitBuffer(scaledviewwidth, viewheight);
    R_InitTextureMapping();

    {
        let mut sprites = SPRITES.borrow_mut();

        // psprite scales
        sprites.pspritescale = FRACUNIT * viewwidth / SCREENWIDTH as i32;
        sprites.pspriteiscale = FRACUNIT * SCREENWIDTH as i32 / viewwidth;

        // thing clipping
        for i in 0..viewwidth as usize {
            sprites.screenheightarray[i] = viewheight as i16;
        }
    }

    let colormaps = RENDER_DATA.borrow().colormaps;
    let mut view = VIEW.borrow_mut();

    {
        let mut planes = PLANES.borrow_mut();

        // planes
        for i in 0..viewheight as usize {
            let mut dy = ((i as i32 - viewheight / 2) << FRACBITS) + FRACUNIT / 2;
            dy = dy.wrapping_abs();
            planes.yslope[i] = FixedDiv((viewwidth << detailshift) / 2 * FRACUNIT, dy);
        }

        for i in 0..viewwidth as usize {
            let cosadj = (*math::tables::finecosine
                .0
                .add((view.xtoviewangle[i] >> ANGLETOFINESHIFT) as usize))
            .wrapping_abs();
            planes.distscale[i] = FixedDiv(FRACUNIT, cosadj);
        }
    }

    // Calculate the light levels to use for each level / scale combination.
    for i in 0..LIGHTLEVELS {
        let startmap = (((LIGHTLEVELS - 1 - i) * 2) * NUMCOLORMAPS / LIGHTLEVELS) as i32;
        for j in 0..MAXLIGHTSCALE {
            let mut level = startmap
                - (j as i32 * SCREENWIDTH as i32) / (viewwidth << detailshift) / DISTMAP as i32;

            if level < 0 {
                level = 0;
            }
            if level >= NUMCOLORMAPS as i32 {
                level = NUMCOLORMAPS as i32 - 1;
            }

            view.scalelight[i][j] = colormaps.add(level as usize * 256);
        }
    }
}

// ---------------------------------------------------------------------------
// R_Init
// ---------------------------------------------------------------------------

/// One-time renderer initialisation called at engine startup.
///
/// Calls, in order: `R_InitData`, `R_InitPointToAngle`, `R_InitTables`,
/// `R_SetViewSize`, `R_InitPlanes`, `R_InitLightTables`, `R_InitSkyMap`,
/// `R_InitTranslationTables`. Prints a `.` to stdout after each step as a
/// startup progress indicator.
///
/// Equivalent to `R_Init` in `r_main.c`.
///
/// # Safety
/// Initialises global renderer state; must be called exactly once before any
/// frame is rendered. Calls multiple unsafe initialisers internally.
pub unsafe fn R_Init() {
    R_InitData();
    print!(".");
    R_InitPointToAngle();
    print!(".");
    R_InitTables();
    print!(".");
    let (screenblocks, detail_level) = {
        let menu = MENU.borrow();
        (menu.screenblocks, menu.detailLevel)
    };
    R_SetViewSize(screenblocks, detail_level);
    R_InitPlanes();
    print!(".");
    R_InitLightTables();
    print!(".");
    crate::engine::render_3d::sky::R_InitSkyMap();
    R_InitTranslationTables();
    print!(".");

    VIEW.borrow_mut().framecount = 0;
}

// ---------------------------------------------------------------------------
// R_PointInSubsector
// ---------------------------------------------------------------------------

/// Node flag indicating the child index refers to a subsector, not another
/// node. Matches `NF_SUBSECTOR` in `r_local.h`.
const NF_SUBSECTOR: u32 = 0x8000;

/// Walk the BSP tree to find the subsector that contains the given map point.
///
/// Starts at the root node (the last one) and descends by calling
/// [`R_PointOnSide`] at each node until reaching a leaf (subsector) indicated
/// by the `NF_SUBSECTOR` flag. Handles the degenerate case of a single
/// subsector (no nodes).
///
/// Equivalent to `R_PointInSubsector` in `r_main.c`.
///
/// Reads the level's `nodes` (a level must be loaded), borrowing the level
/// for the walk.
pub fn R_PointInSubsector(x: fixed_t, y: fixed_t) -> SubsectorId {
    let level = LEVEL.borrow();
    let nodes = &level.nodes[..];
    // single subsector is a special case
    if nodes.is_empty() {
        return SubsectorId::new(0);
    }

    let mut nodenum = nodes.len() as i32 - 1;

    while (nodenum as u32) & NF_SUBSECTOR == 0 {
        let node = &nodes[nodenum as usize];
        let side = R_PointOnSide(x, y, node);
        nodenum = node.children[side as usize] as i32;
    }

    SubsectorId::new((nodenum as u32 & !NF_SUBSECTOR) as usize)
}

// ---------------------------------------------------------------------------
// R_SetupFrame
// ---------------------------------------------------------------------------

/// Prepare all per-frame view state from the given player's current state.
///
/// Sets `viewplayer`, `viewx`, `viewy`, `viewz`, `viewangle`, `viewsin`,
/// `viewcos`, `extralight`, and `fixedcolormap`. Also resets `sscount` and
/// increments both `framecount` and [`VALIDCOUNT`].
///
/// When the player has a fixed colormap powerup, fills `scalelightfixed`
/// with the fixed colormap pointer and redirects the wall state's
/// `walllights` to it so that wall-lighting lookup code requires no
/// special-case handling.
///
/// Equivalent to `R_SetupFrame` in `r_main.c`.
///
/// # Safety
/// `player` must be a valid, non-null pointer to a [`PlayerT`] whose `mo`
/// mobj pointer is also valid. Reads and writes numerous renderer state fields.
pub unsafe fn R_SetupFrame(player: *mut PlayerT) {
    let colormaps = RENDER_DATA.borrow().colormaps;
    let mut view = VIEW.borrow_mut();

    view.viewplayer = player;
    let mo = (*player).mo().ptr();
    view.viewx = (*mo).x;
    view.viewy = (*mo).y;
    view.viewangle = (*mo).angle.wrapping_add(view.viewangleoffset as u32);
    view.extralight = (*player).extralight;

    view.viewz = (*player).viewz;

    view.viewsin = math::tables::finesine[(view.viewangle >> ANGLETOFINESHIFT) as usize];
    view.viewcos = *math::tables::finecosine
        .0
        .add((view.viewangle >> ANGLETOFINESHIFT) as usize);

    view.sscount = 0;

    if (*player).fixedcolormap != 0 {
        view.fixedcolormap = colormaps
            .add((*player).fixedcolormap as usize * 256 * std::mem::size_of::<lighttable_t>());

        let fixedcolormap = view.fixedcolormap;
        for i in 0..MAXLIGHTSCALE {
            view.scalelightfixed[i] = fixedcolormap;
        }
        WALL.borrow_mut().walllights = scalelightfixed_row();
    } else {
        view.fixedcolormap = ptr::null_mut();
    }

    view.framecount += 1;
    VALIDCOUNT.set(VALIDCOUNT.get() + 1);
}

// ---------------------------------------------------------------------------
// R_RenderPlayerView
// ---------------------------------------------------------------------------

/// Top-level per-frame render entry point.
///
/// Calls [`R_SetupFrame`] to prepare the view state, clears all renderer
/// buffers (`R_ClearClipSegs`, `R_ClearDrawSegs`, `R_ClearPlanes`,
/// `R_ClearSprites`), traverses the BSP tree via `R_RenderBSPNode`, then
/// draws floors/ceilings (`R_DrawPlanes`) and masked objects
/// (`R_DrawMasked`). `NetUpdate` is called between phases to keep network
/// and demo state responsive on slow machines.
///
/// Equivalent to `R_RenderPlayerView` (`R_RenderView`) in `r_main.c`.
///
/// # Safety
/// `player` must be a valid, non-null pointer to a fully-initialised
/// [`PlayerT`]. All renderer state and map data must have been loaded and
/// initialised prior to this call.
pub unsafe fn R_RenderPlayerView(player: *mut PlayerT) {
    R_SetupFrame(player);

    R_ClearClipSegs();
    R_ClearDrawSegs();
    R_ClearPlanes();
    R_ClearSprites();

    NetUpdate();
    {
        // The traversal borrows the level once and hands it down.
        let mut level = LEVEL.borrow_mut();
        let root_node = level.nodes.len() as i32 - 1;
        R_RenderBSPNode(&mut level, root_node);
    }
    NetUpdate();
    R_DrawPlanes();
    NetUpdate();
    R_DrawMasked();
    NetUpdate();
}

// ---------------------------------------------------------------------------
// Regression tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use {super::*, crate::engine::math::fixed::FixedDiv, std::sync::Mutex};

    /// Process-wide guard serialising tests that read or write the view
    /// position in [`VIEW`]. `cargo test` runs tests in parallel by default;
    /// without this lock, two tests touching it could race and tear each
    /// other's state.
    static RENDER_GLOBALS_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that snapshots `viewx`/`viewy` on construction and
    /// restores them on drop, so an assertion panic cannot leave the
    /// view state perturbed for any subsequent test that happens to
    /// acquire the lock.
    struct ViewPosGuard {
        saved_x: fixed_t,
        saved_y: fixed_t,
    }

    impl ViewPosGuard {
        fn new() -> Self {
            let view = VIEW.borrow();
            Self {
                saved_x: view.viewx,
                saved_y: view.viewy,
            }
        }
    }

    impl Drop for ViewPosGuard {
        fn drop(&mut self) {
            let mut view = VIEW.borrow_mut();
            view.viewx = self.saved_x;
            view.viewy = self.saved_y;
        }
    }

    /// Set the view position in [`VIEW`].
    fn set_view_position(x: fixed_t, y: fixed_t) {
        let mut view = VIEW.borrow_mut();
        view.viewx = x;
        view.viewy = y;
    }

    /// The DBITS constant was once incorrectly set to 15 instead of
    /// FRACBITS - SLOPEBITS = 16 - 11 = 5.  With the wrong value,
    /// R_PointToDist indexes tantoangle with the wrong shift and
    /// computes completely wrong distances, causing wall-offset /
    /// sprite-clipping glitches.
    #[test]
    fn dbits_is_five() {
        assert_eq!(DBITS, 5, "DBITS must be FRACBITS - SLOPEBITS = 5");
    }

    /// For a 45-degree line (dy == dx), FixedDiv(dy, dx) returns FRACUNIT.
    /// With DBITS = 5 the index into tantoangle is FRACUNIT >> 5 == 2048,
    /// the top of the table.  With DBITS = 15 the index would be 2,
    /// which produces a garbage distance.
    #[test]
    fn r_point_to_dist_45_degree_index() {
        let frac = FixedDiv(FRACUNIT, FRACUNIT); // dy == dx
        let index = (frac as u32 >> DBITS) as usize;
        assert_eq!(
            index, 2048,
            "frac>>DBITS for 45-degree case must index tantoangle[2048]"
        );
    }

    #[test]
    fn r_point_to_dist_reasonable_values() {
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();
        set_view_position(0, 0);
        // Straight ahead: distance should be approximately FRACUNIT.
        let dist_ahead = R_PointToDist(FRACUNIT, 0);
        assert!(dist_ahead > 0, "distance straight ahead must be positive");
        assert!(
            (dist_ahead - FRACUNIT).abs() < FRACUNIT / 4,
            "distance straight ahead should be near FRACUNIT, got {}",
            dist_ahead
        );

        // 45-degree diagonal: distance should be larger than straight ahead.
        let dist_diag = R_PointToDist(FRACUNIT, FRACUNIT);
        assert!(
            dist_diag > dist_ahead,
            "diagonal distance ({}) must exceed straight-ahead distance ({})",
            dist_diag,
            dist_ahead
        );

        // Same point: distance must be exactly zero.
        let dist_zero = R_PointToDist(0, 0);
        assert_eq!(dist_zero, 0, "distance to view position must be zero");
    }

    #[test]
    fn r_point_to_angle2_cardinals() {
        // Due east is exact.
        assert_eq!(R_PointToAngle2(0, 0, FRACUNIT, 0), 0);

        // Due north quantizes to ANG90 - 1 in the fixed-point LUT.
        assert_eq!(R_PointToAngle2(0, 0, 0, FRACUNIT), ANG90 - 1);

        // Due west quantizes to ANG180 - 1.
        assert_eq!(R_PointToAngle2(0, 0, -FRACUNIT, 0), ANG180 - 1);

        // Due south is exact.
        assert_eq!(R_PointToAngle2(0, 0, 0, -FRACUNIT), ANG270);
    }

    /// R_PointToAngle2 used to set `viewx = x1; viewy = y1` and delegate to
    /// R_PointToAngle, clobbering the view position. The refactored
    /// implementation must compute the angle purely from the delta and leave
    /// `viewx` / `viewy` untouched.
    #[test]
    fn r_point_to_angle2_preserves_view_globals() {
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();
        set_view_position(12345, -6789);

        let _ = R_PointToAngle2(1000, 2000, 3000, 4000);

        let view = VIEW.borrow();
        assert_eq!(view.viewx, 12345, "R_PointToAngle2 must not modify viewx");
        assert_eq!(view.viewy, -6789, "R_PointToAngle2 must not modify viewy");
    }

    /// R_PointToAngle2 must be translation-invariant: the angle from
    /// `(x1, y1)` to `(x2, y2)` equals the angle of the delta vector from
    /// the origin. Confirms the rewrite preserves the original semantics
    /// for nonzero source points.
    #[test]
    fn r_point_to_angle2_translation_invariant() {
        // R_PointToAngle2 itself does not read viewx/viewy, but acquire
        // the lock and restore guard anyway so this test never observes
        // a torn state if a future change introduces a view-state read.
        let _g = RENDER_GLOBALS_LOCK.lock().unwrap();
        let _vp = ViewPosGuard::new();

        let cases: [(fixed_t, fixed_t); 8] = [
            (FRACUNIT, 0),
            (FRACUNIT, FRACUNIT),
            (0, FRACUNIT),
            (-FRACUNIT, FRACUNIT),
            (-FRACUNIT, 0),
            (-FRACUNIT, -FRACUNIT),
            (0, -FRACUNIT),
            (FRACUNIT, -FRACUNIT),
        ];

        for (dx, dy) in cases.iter().copied() {
            let from_origin = R_PointToAngle2(0, 0, dx, dy);
            let translated = R_PointToAngle2(
                100 * FRACUNIT,
                -50 * FRACUNIT,
                100 * FRACUNIT + dx,
                -50 * FRACUNIT + dy,
            );
            assert_eq!(
                from_origin, translated,
                "R_PointToAngle2 must be translation-invariant (dx={}, dy={})",
                dx, dy
            );
        }
    }

    /// R_ScaleFromGlobalAngle previously used plain `+` and `-` on
    /// angle_t values, which panics in debug builds on wraparound.
    /// It must use wrapping_add / wrapping_sub.
    #[test]
    fn r_scale_from_global_angle_wraparound_no_panic() {
        let mut view = ViewState::new();
        view.viewangle = 0;
        view.projection = FRACUNIT;
        view.detailshift = 0;
        let mut wall = WallState::new();
        wall.rw_normalangle = 0;
        wall.rw_distance = FRACUNIT;

        // Angles near the u32 boundary must not panic.
        let _ = R_ScaleFromGlobalAngle(&view, &wall, u32::MAX);
        let _ = R_ScaleFromGlobalAngle(&view, &wall, 0);
        let _ = R_ScaleFromGlobalAngle(&view, &wall, ANG90);
        let _ = R_ScaleFromGlobalAngle(&view, &wall, ANG180);
        let _ = R_ScaleFromGlobalAngle(&view, &wall, ANG270);
        let _ = R_ScaleFromGlobalAngle(&view, &wall, view.viewangle.wrapping_sub(1));
        let _ = R_ScaleFromGlobalAngle(&view, &wall, view.viewangle.wrapping_add(1));
    }

    #[test]
    fn r_point_in_subsector_walks_the_nodes_to_a_leaf() {
        let _guard = crate::engine::play::setup::LEVEL_TEST_LOCK.lock().unwrap();
        // One node on the vertical line x = 0 pointing north: the right
        // (front) child is subsector 1, the left (back) child subsector 0.
        let node = node_t {
            x: 0,
            y: 0,
            dx: 0,
            dy: FRACUNIT,
            bbox: [[0; 4]; 2],
            children: [NF_SUBSECTOR as u16 | 1, NF_SUBSECTOR as u16],
        };
        let saved_nodes = std::mem::take(&mut LEVEL.borrow_mut().nodes);
        // Without nodes the level is a single subsector.
        assert_eq!(R_PointInSubsector(5 * FRACUNIT, 0), SubsectorId::new(0));

        LEVEL.borrow_mut().nodes = vec![node];
        assert_eq!(R_PointInSubsector(5 * FRACUNIT, 0), SubsectorId::new(1));
        assert_eq!(R_PointInSubsector(-5 * FRACUNIT, 0), SubsectorId::new(0));
        LEVEL.borrow_mut().nodes = saved_nodes;
    }

    #[test]
    fn r_point_on_side_vertical_line() {
        // Node with dx == 0, dy > 0 (vertical line, northward).
        let node = node_t {
            x: 0,
            y: 0,
            dx: 0,
            dy: FRACUNIT,
            bbox: [[0; 4]; 2],
            children: [0; 2],
        };
        // Point to the left of the line -> side 1 (from C logic).
        assert_eq!(R_PointOnSide(-FRACUNIT, 0, &node), 1);
        // Point to the right of the line -> side 0.
        assert_eq!(R_PointOnSide(FRACUNIT, 0, &node), 0);
        // Point exactly on the line -> x <= node.x, side depends on dy > 0 -> 1.
        assert_eq!(R_PointOnSide(0, 0, &node), 1);
    }
}
