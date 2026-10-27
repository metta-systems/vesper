//! Sprite rendering for the Doom software renderer, ported from `vendor/doomgeneric/r_things.c`.
//!
//! Handles projection of map objects (things) into screen-space `vissprite_t` records,
//! depth-sorting them back-to-front (painter's algorithm), clipping each sprite against
//! floor/ceiling silhouettes from the BSP drawseg list, and drawing masked columns for
//! both world sprites and player weapon (psprite) overlays.
//!
//! The C globals live in the renderer's [`SpriteState`] (`sprites`) and,
//! for the vissprite pool that is linked by raw pointers, its
//! [`VisSpriteState`] (`vissprites`). The masked-column inputs
//! (`mfloorclip`, `mceilingclip`, `spryscale`, `sprtopscreen`) are a
//! [`MaskedColumnParams`] block passed by the caller.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]
// The sprite tables and masked column drawing go through raw pointers in
// C's whole-body `unsafe fn` style.
#![allow(unsafe_op_in_unsafe_fn)]

use {crate::i_error, std::ptr};

use crate::engine::{
    arena::Arena,
    doom::player::PspdefT,
    map::SectorId,
    math::{
        fixed::{FRACBITS, FRACUNIT, FixedDiv, FixedMul, fixed_t},
        tables::ANG45,
    },
    play::{info::*, mobj::mobj_t, setup::LevelState},
    world::World,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Screen width in pixels; mirrors `SCREENWIDTH` from `i_video`.
const SCREENWIDTH: usize = crate::engine::interface::video::SCREENWIDTH as usize;

/// Minimum sprite Z distance; sprites closer than this are not projected
/// (MINZ in r_things.c = FRACUNIT × 4).
pub const MINZ: i32 = FRACUNIT * 4;
/// Vertical centre of the screen in pixels, used for sprite projection
/// (BASEYCENTER in r_things.c = 100).
pub const BASEYCENTER: i32 = 100;

/// Maximum number of visible sprites that can be projected in a single frame.
/// Sprites beyond this limit are silently dropped into `overflowsprite`.
const MAXVISSPRITES: usize = 128;

/// Number of distinct light level bands used by the scale-light table.
const LIGHTLEVELS: usize = 16;

/// Right-shift applied to a sector's light level to obtain a band index.
const LIGHTSEGSHIFT: u32 = 4;

/// Maximum scale-light index; caps the luminosity lookup so the table is not
/// over-indexed for very close sprites.
const MAXLIGHTSCALE: usize = 48;

/// Right-shift applied to a sprite's screen scale to produce a `scalelight` index.
const LIGHTSCALESHIFT: u32 = 12;

/// Bitmask that isolates the frame index bits from a state frame field.
/// The upper bit (`FF_FULLBRIGHT`) is stripped so that only the 0-based
/// animation frame number remains.
const FF_FRAMEMASK: i32 = 0x7fff;

/// Flag bit in a state frame field indicating the sprite should be drawn
/// at full brightness regardless of sector lighting.
const FF_FULLBRIGHT: i32 = 0x8000;

/// Index into the player `powers` array for the partial-invisibility power-up.
/// Used when deciding whether to draw the player weapon with a fuzz (shadow)
/// column function.
const pw_invisibility: usize = 2;

/// Silhouette flag: the drawseg has a valid bottom silhouette that can clip
/// sprites from below.
const SIL_BOTTOM: i32 = 1;

/// Silhouette flag: the drawseg has a valid top silhouette that can clip
/// sprites from above.
const SIL_TOP: i32 = 2;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One animation frame of a sprite (`spriteframe_t` in `r_defs.h`).
#[derive(Clone, Copy)]
pub struct spriteframe_t {
    /// 0 = use frame 0 for all rotations; 1 = use rotation-specific lumps.
    pub rotate: i32,
    /// WAD lump index (relative to firstspritelump) for each of 8 rotations.
    pub lump: [i16; 8],
    /// 1 = horizontally flip the lump for this rotation; 0 = no flip.
    pub flip: [u8; 8],
}

/// Sprite definition table entry for one sprite name (e.g., "TROO")
/// (`spritedef_t` in `r_defs.h`).
/// Points to an array of `spriteframe_t` records, one per animation frame.
#[derive(Clone, Copy)]
pub struct spritedef_t {
    /// Number of animation frames defined for this sprite.
    pub numframes: i32,
    /// Pointer to the heap-allocated array of frame descriptors.
    pub spriteframes: *mut spriteframe_t,
}

/// Visible sprite: a thing that is (partly) visible in the current frame
/// (`vissprite_t` in `r_defs.h`).
///
/// Built by `R_ProjectSprite`; sorted by `scale` (depth) before
/// rasterisation. `colormap` selects light level / palette translation
/// per-column.
#[derive(Clone, Copy)]
pub struct vissprite_t {
    pub prev: *mut vissprite_t,
    pub next: *mut vissprite_t,
    pub x1: i32,
    pub x2: i32,
    pub gx: i32,
    pub gy: i32,
    pub gz: i32,
    pub gzt: i32,
    pub startfrac: i32,
    pub scale: i32,
    pub xiscale: i32,
    pub texturemid: i32,
    pub patch: i32,
    pub colormap: *mut u8,
    pub mobjflags: i32,
}

/// Inputs of [`R_DrawMaskedColumn`]: the C `mfloorclip`, `mceilingclip`,
/// `spryscale` and `sprtopscreen` globals, filled by the sprite and masked
/// mid-texture drawers.
pub struct MaskedColumnParams {
    /// Per-column bottom clip (`screenheightarray`, `clipbot`, or a drawseg
    /// slice of `openings`).
    pub mfloorclip: *mut i16,
    /// Per-column top clip (`negonearray`, `cliptop`, or a drawseg slice of
    /// `openings`).
    pub mceilingclip: *mut i16,
    /// Current sprite Y scale.
    pub spryscale: fixed_t,
    /// Screen Y coordinate of the sprite top.
    pub sprtopscreen: fixed_t,
}

/// The 4-character sprite prefix of a lump or sprite name, up to the first
/// NUL: what C's `strncasecmp(a, b, 4)` compares.
fn sprite_prefix(name: &[u8]) -> &[u8] {
    until_nul(&name[..name.len().min(4)])
}

use crate::engine::{
    doom::stat::modifiedgame,
    misc::until_nul,
    render_3d::{
        RENDERER, Renderer,
        data::RenderDataState,
        draw::{ColumnFunction, ColumnParams, DrawState},
        main::{R_PointOnSegSide, R_PointToAngle, ViewState, scalelight_row},
        segs::R_RenderMaskedSegRange,
    },
    wad::{W_GetNumForName, cache_patch_num, format::Column, lumpinfo},
    zone::{PU_CACHE, Z_Malloc},
};

// ---------------------------------------------------------------------------
// State defined by this module
// ---------------------------------------------------------------------------

/// Sprite state (C `r_things.c` globals other than the vissprite pool), the
/// renderer's `sprites`.
pub struct SpriteState {
    /// Scale applied to player weapon (psprite) columns this frame.
    pub pspritescale: fixed_t,
    /// Inverse of pspritescale.
    pub pspriteiscale: fixed_t,
    /// Pointer to the active light-table row for sprites this frame.
    pub spritelights: *const *mut u8,
    /// Clipping array initialised to -1 for psprite bottom clipping. The
    /// drawsegs and the psprite drawer point into it.
    pub negonearray: [i16; SCREENWIDTH],
    /// Clipping array initialised to viewheight for psprite top clipping.
    /// The drawsegs and the psprite drawer point into it.
    pub screenheightarray: [i16; SCREENWIDTH],
    /// Pointer to the sprite definition table, `numsprites` entries.
    pub sprites: *mut spritedef_t,
    /// Total number of sprite names found in the WAD.
    pub numsprites: i32,
    /// Temporary frame-building array used during R_InitSprites.
    pub sprtemp: [spriteframe_t; 29],
    /// Highest frame index seen for the current sprite during R_InitSprites.
    pub maxframe: i32,
    /// Name of the sprite currently being processed by R_InitSprites.
    pub spritename: &'static str,
}

impl SpriteState {
    /// The state before `R_InitSprites`: no sprite table, zeroed arrays.
    pub const fn new() -> Self {
        Self {
            pspritescale: 0,
            pspriteiscale: 0,
            spritelights: ptr::null(),
            negonearray: [0; SCREENWIDTH],
            screenheightarray: [0; SCREENWIDTH],
            sprites: ptr::null_mut(),
            numsprites: 0,
            sprtemp: [spriteframe_t {
                rotate: 0,
                lump: [0; 8],
                flip: [0; 8],
            }; 29],
            maxframe: 0,
            spritename: "",
        }
    }
}

impl Default for SpriteState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Vissprite pool
// ---------------------------------------------------------------------------

/// The per-frame vissprite pool and sprite clip scratch arrays, the
/// renderer's `vissprites`.
///
/// The sorted list links the pool entries and `vsprsortedhead` by raw
/// pointer, and `mfloorclip`/`mceilingclip` point into `clipbot`/`cliptop`;
/// the masked phase derives those pointers from the renderer it is handed.
pub struct VisSpriteState {
    /// Fixed-size pool of projected sprite records for the current frame.
    /// Accessed sequentially via `vissprite_p`; up to `MAXVISSPRITES` entries.
    vissprites: [vissprite_t; MAXVISSPRITES],
    /// Index of the next free slot in `vissprites` (the C pointer
    /// `vissprite_p`); advanced by `R_NewVisSprite`.
    vissprite_p: usize,
    /// Sentinel vissprite returned when the pool is exhausted so callers always
    /// receive a valid (though discarded) write target.
    overflowsprite: vissprite_t,
    /// Dummy head node of the doubly-linked sorted vissprite list built by
    /// `R_SortVisSprites`; iterated by `R_DrawMasked`.
    vsprsortedhead: vissprite_t,
    /// Per-column bottom clip built by `R_DrawSprite` (the C function-local
    /// `clipbot`).
    clipbot: [i16; SCREENWIDTH],
    /// Per-column top clip built by `R_DrawSprite` (the C function-local
    /// `cliptop`).
    cliptop: [i16; SCREENWIDTH],
}

impl VisSpriteState {
    /// An empty pool.
    pub const fn new() -> Self {
        Self {
            vissprites: unsafe { std::mem::zeroed() },
            vissprite_p: 0,
            overflowsprite: unsafe { std::mem::zeroed() },
            vsprsortedhead: unsafe { std::mem::zeroed() },
            clipbot: [0; SCREENWIDTH],
            cliptop: [0; SCREENWIDTH],
        }
    }
}

impl Default for VisSpriteState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// R_InstallSpriteLump
// ---------------------------------------------------------------------------

/// Record one WAD lump as a rotation of a sprite frame in `sprtemp`.
///
/// Called exclusively from `R_InitSpriteDefs` for each lump whose name
/// matches the current sprite being processed. If `rotation == 0` the lump
/// is installed for all eight rotations; otherwise it fills exactly one slot.
/// Calls `i_error!` on duplicate or inconsistent assignments.
fn R_InstallSpriteLump(
    sprite_state: &mut SpriteState,
    firstspritelump: i32,
    lump: i32,
    frame: u32,
    rotation: u32,
    flipped: i32,
) {
    if frame >= 29 || rotation > 8 {
        i_error!("R_InstallSpriteLump: Bad frame characters in lump {}", lump);
    }

    if frame as i32 > sprite_state.maxframe {
        sprite_state.maxframe = frame as i32;
    }

    let spritename = sprite_state.spritename;
    let sprframe = &mut sprite_state.sprtemp[frame as usize];

    if rotation == 0 {
        // The lump should be used for all rotations.
        if sprframe.rotate == 0 {
            i_error!(
                "R_InitSprites: Sprite {} frame {} has multip rot=0 lump",
                spritename,
                char::from(b'A' + frame as u8)
            );
        }
        if sprframe.rotate == 1 {
            i_error!(
                "R_InitSprites: Sprite {} frame {} has rotations and a rot=0 lump",
                spritename,
                char::from(b'A' + frame as u8)
            );
        }
        sprframe.rotate = 0;
        for r in 0..8 {
            sprframe.lump[r] = (lump - firstspritelump) as i16;
            sprframe.flip[r] = flipped as u8;
        }
        return;
    }

    // The lump is only used for one rotation.
    if sprframe.rotate == 0 {
        i_error!(
            "R_InitSprites: Sprite {} frame {} has rotations and a rot=0 lump",
            spritename,
            char::from(b'A' + frame as u8)
        );
    }

    sprframe.rotate = 1;

    // Make 0 based.
    let rot = (rotation - 1) as usize;
    if sprframe.lump[rot] != -1 {
        i_error!(
            "R_InitSprites: Sprite {} : {} : {} has two lumps mapped to it",
            spritename,
            char::from(b'A' + frame as u8),
            char::from(b'1' + rot as u8)
        );
    }

    sprframe.lump[rot] = (lump - firstspritelump) as i16;
    sprframe.flip[rot] = flipped as u8;
}

// ---------------------------------------------------------------------------
// R_InitSpriteDefs
// ---------------------------------------------------------------------------

/// Build the `sprites` lookup table from a null-terminated list of 4-character
/// sprite names.
///
/// Scans every sprite-range WAD lump, decodes frame letter and rotation digit
/// from the lump name, and calls `R_InstallSpriteLump` to populate `sprtemp`.
/// After processing all lumps for a sprite, validates that every referenced
/// frame has a complete rotation set, then allocates a permanent `spriteframe_t`
/// array and copies `sprtemp` into it.
///
/// In a modified game (`modifiedgame` true) lump numbers are resolved through
/// `W_GetNumForName` to respect WAD replacement ordering.
///
/// # Safety
/// `firstspritelump` and `lastspritelump` of `data` must already be
/// initialised before this function is called.
unsafe fn R_InitSpriteDefs(
    sprite_state: &mut SpriteState,
    data: &RenderDataState,
    namelist: &[&'static str],
) {
    sprite_state.numsprites = namelist.len() as i32;

    if sprite_state.numsprites == 0 {
        return;
    }

    sprite_state.sprites = Z_Malloc(
        (sprite_state.numsprites as usize * std::mem::size_of::<spritedef_t>()) as i32,
        1, // PU_STATIC
        ptr::null_mut(),
    ) as *mut spritedef_t;

    let (firstspritelump, lastspritelump) = (data.firstspritelump, data.lastspritelump);
    let start = firstspritelump - 1;
    let end = lastspritelump + 1;

    for i in 0..sprite_state.numsprites {
        sprite_state.spritename = namelist[i as usize];
        ptr::write_bytes(sprite_state.sprtemp.as_mut_ptr(), 0xFF, 29);

        sprite_state.maxframe = -1;

        // Scan the lumps, filling in the frames for whatever is found.
        for l in (start + 1)..end {
            let li = &*lumpinfo().add(l as usize);
            if sprite_prefix(&li.name)
                .eq_ignore_ascii_case(sprite_prefix(sprite_state.spritename.as_bytes()))
            {
                let frame = (li.name[4] - b'A') as u32;
                let rotation = (li.name[5] - b'0') as u32;

                let patched = if modifiedgame().is_truthy() {
                    W_GetNumForName(&li.name)
                } else {
                    l
                };

                R_InstallSpriteLump(sprite_state, firstspritelump, patched, frame, rotation, 0);

                if li.name[6] != 0 {
                    let frame = (li.name[6] - b'A') as u32;
                    let rotation = (li.name[7] - b'0') as u32;
                    R_InstallSpriteLump(sprite_state, firstspritelump, l, frame, rotation, 1);
                }
            }
        }

        // Check the frames that were found for completeness.
        if sprite_state.maxframe == -1 {
            let spr = &mut *sprite_state.sprites.add(i as usize);
            spr.numframes = 0;
            continue;
        }

        sprite_state.maxframe += 1;
        let maxframe = sprite_state.maxframe;
        let spritename = sprite_state.spritename;

        for frame in 0..maxframe {
            match sprite_state.sprtemp[frame as usize].rotate {
                -1 => {
                    // No rotations were found for that frame at all.
                    i_error!(
                        "R_InitSprites: No patches found for {} frame {}",
                        spritename,
                        char::from(b'A' + frame as u8)
                    );
                }
                0 => {
                    // Only the first rotation is needed.
                }
                1 => {
                    // Must have all 8 frames.
                    for rotation in 0..8 {
                        if sprite_state.sprtemp[frame as usize].lump[rotation] == -1 {
                            i_error!(
                                "R_InitSprites: Sprite {} frame {} is missing rotations",
                                spritename,
                                char::from(b'A' + frame as u8)
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        // Allocate space for the frames present and copy sprtemp to it.
        let spr = &mut *sprite_state.sprites.add(i as usize);
        spr.numframes = maxframe;
        spr.spriteframes = Z_Malloc(
            (maxframe as usize * std::mem::size_of::<spriteframe_t>()) as i32,
            1, // PU_STATIC
            ptr::null_mut(),
        ) as *mut spriteframe_t;
        ptr::copy_nonoverlapping(
            sprite_state.sprtemp.as_ptr(),
            spr.spriteframes,
            maxframe as usize,
        );
    }
}

// ---------------------------------------------------------------------------
// R_InitSprites
// ---------------------------------------------------------------------------

/// Initialise the sprite system at program start.
///
/// Fills `negonearray` with `-1` (used as the ceiling clip sentinel for psprites),
/// then delegates to `R_InitSpriteDefs` to build the sprite frame lookup table.
/// # Safety
/// `namelist` holds the 4-character sprite names. Must be called after WAD
/// loading (`W_InitMultipleFiles`) has set up `firstspritelump` and
/// `lastspritelump`.
pub unsafe fn R_InitSprites(namelist: &[&'static str]) {
    let mut renderer = RENDERER.borrow_mut();
    let Renderer { sprites, data, .. } = &mut *renderer;
    for i in 0..SCREENWIDTH {
        sprites.negonearray[i] = -1;
    }
    R_InitSpriteDefs(sprites, data, namelist);
}

// ---------------------------------------------------------------------------
// R_ClearSprites
// ---------------------------------------------------------------------------

/// Reset the vissprite pool to empty at the start of each frame.
///
/// Resets `vissprite_p` to the beginning of the `vissprites` array so that
/// the next frame can overwrite all previous entries. Called once per frame
/// in the frame's setup phase, before any call to `R_AddSprites` or
/// `R_ProjectSprite`.
pub fn R_ClearSprites(pool: &mut VisSpriteState) {
    pool.vissprite_p = 0;
}

// ---------------------------------------------------------------------------
// R_NewVisSprite
// ---------------------------------------------------------------------------

/// Allocate the next vissprite slot from the pool and return it.
///
/// If the pool is full (`MAXVISSPRITES` entries already allocated this frame)
/// returns `overflowsprite` so the caller can write without crashing; the
/// overflow record is silently discarded. The slot belongs to the frame
/// until the next `R_ClearSprites` call.
pub fn R_NewVisSprite(pool: &mut VisSpriteState) -> &mut vissprite_t {
    if pool.vissprite_p == MAXVISSPRITES {
        return &mut pool.overflowsprite;
    }
    pool.vissprite_p += 1;
    &mut pool.vissprites[pool.vissprite_p - 1]
}

// ---------------------------------------------------------------------------
// R_DrawMaskedColumn
// ---------------------------------------------------------------------------

/// Draw one vertical column of a masked (partly transparent) sprite or
/// mid-texture using `draw_column`.
///
/// Iterates over the posts of `column`, clipping each post against
/// `mfloorclip` and `mceilingclip` before invoking `draw_column`. Used for both
/// world sprites (via `R_DrawVisSprite`) and masked mid-textures
/// (via `R_RenderMaskedSegRange`).
/// Called from `r_segs.rs`.
///
/// # Safety
/// - `masked` and `params.dc_x` must be valid for the current column being
///   drawn.
/// - `draw_column` reads each post through `dc_source`, as in C, so the
///   column's bytes must stay valid until the call returns.
pub unsafe fn R_DrawMaskedColumn(
    column: Column<'_>,
    masked: &MaskedColumnParams,
    params: &mut ColumnParams,
    draw: &mut DrawState,
    draw_column: ColumnFunction,
) {
    let basetexturemid = params.dc_texturemid;
    let MaskedColumnParams {
        mfloorclip,
        mceilingclip,
        spryscale,
        sprtopscreen,
    } = *masked;

    for post in column {
        // Calculate unclipped screen coordinates for post.
        let topscreen = sprtopscreen + spryscale * post.topdelta as i32;
        let bottomscreen = topscreen + spryscale * post.pixels.len() as i32;

        params.dc_yl = (topscreen + FRACUNIT - 1) >> FRACBITS;
        params.dc_yh = (bottomscreen - 1) >> FRACBITS;

        if params.dc_yh >= *mfloorclip.add(params.dc_x as usize) as i32 {
            params.dc_yh = *mfloorclip.add(params.dc_x as usize) as i32 - 1;
        }
        if params.dc_yl <= *mceilingclip.add(params.dc_x as usize) as i32 {
            params.dc_yl = *mceilingclip.add(params.dc_x as usize) as i32 + 1;
        }

        if params.dc_yl <= params.dc_yh {
            params.dc_source = post.pixels.as_ptr();
            params.dc_texturemid = basetexturemid - ((post.topdelta as i32) << FRACBITS);

            draw_column(params, draw);
        }
    }

    params.dc_texturemid = basetexturemid;
}

// ---------------------------------------------------------------------------
// R_DrawVisSprite
// ---------------------------------------------------------------------------

/// Draw a fully projected vissprite to the screen.
///
/// Sets up the column parameters (`dc_colormap`, `dc_iscale`,
/// `dc_texturemid`) and `spryscale`/`sprtopscreen` from the vissprite fields,
/// then iterates over each screen column from `vis.x1` to `vis.x2`, locating
/// the corresponding patch column and calling `R_DrawMaskedColumn`.
///
/// The column drawer is `fuzzcolfunc` for shadow-drawn sprites (null
/// colormap), `transcolfunc` for translated sprites, and `colfunc` otherwise.
///
/// `masked.mfloorclip` and `masked.mceilingclip` must be set by the caller
/// before this function is invoked.
/// # Safety
/// - `vis` must be a fully initialised `vissprite_t` as produced by
///   `R_ProjectSprite` or `R_DrawPSprite`.
/// - `vis.patch + firstspritelump` must be a sprite patch lump.
/// - `mfloorclip` and `mceilingclip` must be valid arrays of at least
///   `SCREENWIDTH` elements.
pub unsafe fn R_DrawVisSprite(
    data: &RenderDataState,
    view: &ViewState,
    draw: &mut DrawState,
    vis: &vissprite_t,
    masked: &mut MaskedColumnParams,
    _x1: i32,
    _x2: i32,
) {
    // Drawing the sprite allocates nothing from the zone, so the PU_CACHE
    // lump stays cached while the view is used.
    let patch = cache_patch_num(vis.patch + data.firstspritelump, PU_CACHE);

    let mut column = ColumnParams::new(view.centery, data.colormaps);
    column.dc_colormap = vis.colormap;

    let mut draw_column = view.colfunc;
    if column.dc_colormap.is_null() {
        // NULL colormap = shadow draw.
        draw_column = view.fuzzcolfunc;
    } else if vis.mobjflags & MF_TRANSLATION != 0 {
        draw_column = view.transcolfunc;
        column.dc_translation = data
            .colormaps
            .sub(256)
            .add(((vis.mobjflags & MF_TRANSLATION) >> (MF_TRANSSHIFT - 8)) as usize);
    }

    column.dc_iscale = vis.xiscale.abs() >> view.detailshift;
    column.dc_texturemid = vis.texturemid;
    let mut frac = vis.startfrac;
    masked.spryscale = vis.scale;
    masked.sprtopscreen = view.centeryfrac - FixedMul(column.dc_texturemid, masked.spryscale);

    for x in vis.x1..=vis.x2 {
        column.dc_x = x;
        let texturecolumn = frac >> FRACBITS;

        #[cfg(feature = "rangecheck")]
        {
            if texturecolumn < 0 || texturecolumn >= patch.width() {
                i_error!("R_DrawSpriteRange: bad texturecolumn");
            }
        }

        R_DrawMaskedColumn(
            patch.column(texturecolumn as usize),
            masked,
            &mut column,
            draw,
            draw_column,
        );

        frac += vis.xiscale;
    }
}

// ---------------------------------------------------------------------------
// R_ProjectSprite
// ---------------------------------------------------------------------------

/// Project a map object (thing) into screen space and, if visible, fill a
/// vissprite record for later drawing.
///
/// Transforms the thing's world position relative to the viewpoint, rejects
/// it if behind the view plane or too far off-axis, selects the correct sprite
/// lump based on the player's viewing angle and the thing's rotation set,
/// computes screen-space left/right column bounds, and writes all drawing
/// parameters (scale, light, texture offsets) into a new `vissprite_t`.
/// Called from `R_AddSprites`.
///
/// # Safety
/// - `sprites`, `numsprites`, `spriteoffset`, `spritewidth`, and
///   `spritetopoffset` must all be initialised by `R_InitSprites` before this
///   function is called.
/// - The view state and `spritelights` must be valid for the current frame.
pub unsafe fn R_ProjectSprite(renderer: &mut Renderer, thing: &mobj_t) {
    let Renderer {
        sprites: sprite_state,
        draw,
        data,
        view,
        vissprites,
        ..
    } = renderer;
    let (sprites, spritelights) = (sprite_state.sprites, sprite_state.spritelights);
    let viewwidth = draw.viewwidth;

    // Transform the origin point.
    let tr_x = thing.x - view.viewx;
    let tr_y = thing.y - view.viewy;

    let gxt = FixedMul(tr_x, view.viewcos);
    let gyt = -FixedMul(tr_y, view.viewsin);

    let tz = gxt - gyt;

    // Thing is behind view plane?
    if tz < MINZ {
        return;
    }

    let xscale = FixedDiv(view.projection, tz);

    let gxt = -FixedMul(tr_x, view.viewsin);
    let gyt = FixedMul(tr_y, view.viewcos);
    let tx = -(gyt + gxt);

    // Too far off the side?
    if tx.abs() > (tz << 2) {
        return;
    }

    // Decide which patch to use for sprite relative to player.
    #[cfg(feature = "rangecheck")]
    {
        if thing.sprite as u32 >= sprite_state.numsprites as u32 {
            i_error!("R_ProjectSprite: invalid sprite number {}", thing.sprite);
        }
    }

    let sprdef = &*sprites.add(thing.sprite as usize);
    #[cfg(feature = "rangecheck")]
    {
        if (thing.frame & FF_FRAMEMASK) >= sprdef.numframes {
            if sprdef.numframes == 0 {
                return; // sprite lump not present in this WAD — skip silently
            }
            i_error!(
                "R_ProjectSprite [THING]: invalid sprite frame {} : {}",
                thing.sprite,
                thing.frame
            );
        }
    }
    let sprframe = &*sprdef
        .spriteframes
        .add((thing.frame & FF_FRAMEMASK) as usize);

    let (lump, flip): (i32, i32);
    if sprframe.rotate == 0 {
        // Use single rotation for all views.
        lump = sprframe.lump[0] as i32;
        flip = sprframe.flip[0] as i32;
    } else {
        // Choose a different rotation based on player view.
        let ang = R_PointToAngle(view, thing.x, thing.y);
        let rot = ((ang.wrapping_sub(thing.angle).wrapping_add((ANG45 / 2) * 9)) >> 29) as usize;
        lump = sprframe.lump[rot] as i32;
        flip = sprframe.flip[rot] as i32;
    }

    // Calculate edges of the shape.
    let mut tx = tx - *data.spriteoffset.add(lump as usize);
    let x1 = (view.centerxfrac + FixedMul(tx, xscale)) >> FRACBITS;

    // Off the right side?
    if x1 > viewwidth {
        return;
    }

    tx += *data.spritewidth.add(lump as usize);
    let x2 = ((view.centerxfrac + FixedMul(tx, xscale)) >> FRACBITS) - 1;

    // Off the left side.
    if x2 < 0 {
        return;
    }

    // Store information in a vissprite.
    let vis = R_NewVisSprite(vissprites);
    vis.mobjflags = thing.flags;
    vis.scale = xscale << view.detailshift;
    vis.gx = thing.x;
    vis.gy = thing.y;
    vis.gz = thing.z;
    vis.gzt = thing.z + *data.spritetopoffset.add(lump as usize);
    vis.texturemid = vis.gzt - view.viewz;
    vis.x1 = if x1 < 0 { 0 } else { x1 };
    vis.x2 = if x2 >= viewwidth { viewwidth - 1 } else { x2 };
    let iscale = FixedDiv(FRACUNIT, xscale);

    if flip != 0 {
        vis.startfrac = *data.spritewidth.add(lump as usize) - 1;
        vis.xiscale = -iscale;
    } else {
        vis.startfrac = 0;
        vis.xiscale = iscale;
    }

    if vis.x1 > x1 {
        vis.startfrac += vis.xiscale * (vis.x1 - x1);
    }
    vis.patch = lump;

    // Get light level.
    if thing.flags & MF_SHADOW != 0 {
        // Shadow draw.
        vis.colormap = ptr::null_mut();
    } else if !view.fixedcolormap.is_null() {
        // Fixed map.
        vis.colormap = view.fixedcolormap;
    } else if thing.frame & FF_FULLBRIGHT != 0 {
        // Full bright.
        vis.colormap = data.colormaps;
    } else {
        // Diminished light.
        let mut index = (xscale >> (LIGHTSCALESHIFT - view.detailshift as u32)) as usize;
        if index >= MAXLIGHTSCALE {
            index = MAXLIGHTSCALE - 1;
        }
        vis.colormap = *spritelights.add(index);
    }
}

/// The `scalelight` row of `view` for a sector light level plus
/// `extralight`, clamped to the table.
fn sprite_light_row(view: &ViewState, lightlevel: i16, extralight: i32) -> *const *mut u8 {
    let lightnum = (lightlevel >> LIGHTSEGSHIFT as i16) as i32 + extralight;

    if lightnum < 0 {
        scalelight_row(view, 0)
    } else if lightnum >= LIGHTLEVELS as i32 {
        scalelight_row(view, LIGHTLEVELS - 1)
    } else {
        scalelight_row(view, lightnum as usize)
    }
}

// ---------------------------------------------------------------------------
// R_AddSprites
// ---------------------------------------------------------------------------

/// Add all things in a sector to the vissprite list during BSP traversal.
///
/// Called once per unique sector encountered while traversing the BSP tree.
/// Uses `sec.validcount` to skip sectors that were already processed this
/// frame (a sector can appear in multiple subsectors). Sets `spritelights`
/// from the sector's light level, then calls `R_ProjectSprite` for every
/// thing in the sector's thing list. `level`, `mobjs` and `validcount` (the
/// frame's stamp, the world's `validcount`) come from the BSP traversal's
/// borrow of the world.
///
/// # Safety
/// - `sec` must be a sector of `level` whose thing list links map objects
///   of `mobjs`.
/// - `scalelight` and `extralight` must be initialised for the current
///   frame before this function is called.
pub unsafe fn R_AddSprites(
    renderer: &mut Renderer,
    level: &mut LevelState,
    mobjs: &Arena<mobj_t>,
    validcount: i32,
    sec: SectorId,
) {
    let sec = &mut level.sectors[sec];

    // BSP is traversed by subsector.
    // A sector might have been split into several subsectors during BSP building.
    // Thus we check whether it's already added.
    if sec.validcount == validcount {
        return;
    }

    // Well, now it will be done.
    sec.validcount = validcount;

    let view = &renderer.view;
    renderer.sprites.spritelights = sprite_light_row(view, sec.lightlevel, view.extralight);

    // Handle all things in sector.
    let mut thing = sec.thinglist;
    while let Some(current) = thing {
        let current = &mobjs[current];
        R_ProjectSprite(renderer, current);
        thing = current.snext;
    }
}

// ---------------------------------------------------------------------------
// R_DrawPSprite
// ---------------------------------------------------------------------------

/// Draw one player weapon sprite (psprite) onto the screen.
///
/// Psprites are rendered in screen (HUD) space rather than world space:
/// the horizontal position is based on `psp.sx` offset from the screen
/// centre and the vertical position on `psp.sy` relative to `BASEYCENTER`.
/// A temporary stack-allocated `vissprite_t` (`avis`) is filled and passed
/// directly to `R_DrawVisSprite`; it is never inserted into the vissprite pool.
///
/// If the view player carries the partial-invisibility power-up
/// (`invisibility` tics left) at a sufficient level the weapon is drawn
/// with `fuzzcolfunc` (shadow effect).
/// # Safety
/// - `sprites`, `pspritescale`, `pspriteiscale`, `spritelights`, and the
///   clip pointers in `masked` must all be valid for the current frame.
pub unsafe fn R_DrawPSprite(
    renderer: &mut Renderer,
    invisibility: i32,
    psp: &PspdefT,
    masked: &mut MaskedColumnParams,
) {
    // Decide which patch to use.
    if psp.state.is_null() {
        return;
    }

    let state = psp.state.get();

    let Renderer {
        sprites: sprite_state,
        draw,
        data,
        view,
        ..
    } = renderer;
    let (sprites, spritelights, pspritescale, pspriteiscale) = (
        sprite_state.sprites,
        sprite_state.spritelights,
        sprite_state.pspritescale,
        sprite_state.pspriteiscale,
    );

    #[cfg(feature = "rangecheck")]
    {
        let sprite = state.sprite;
        if sprite as u32 >= sprite_state.numsprites as u32 {
            i_error!("R_ProjectSprite: invalid sprite number {}", sprite);
        }
    }

    let sprdef = &*sprites.add(state.sprite as usize);
    #[cfg(feature = "rangecheck")]
    {
        if (state.frame & FF_FRAMEMASK) >= sprdef.numframes {
            i_error!(
                "R_ProjectSprite [PSPRITE]: invalid sprite frame {} : {}",
                state.sprite,
                state.frame
            );
        }
    }
    let sprframe = &*sprdef
        .spriteframes
        .add((state.frame & FF_FRAMEMASK) as usize);

    let lump = sprframe.lump[0] as i32;
    let flip = sprframe.flip[0] as i32;

    let viewwidth = draw.viewwidth;
    let (centerxfrac, detailshift, fixedcolormap) =
        (view.centerxfrac, view.detailshift, view.fixedcolormap);
    let (spriteoffset, spritewidth, spritetopoffset, colormaps) = (
        data.spriteoffset,
        data.spritewidth,
        data.spritetopoffset,
        data.colormaps,
    );

    // Calculate edges of the shape.
    let mut tx = psp.sx - 160 * FRACUNIT;
    tx -= *spriteoffset.add(lump as usize);
    let x1 = (centerxfrac + FixedMul(tx, pspritescale)) >> FRACBITS;

    // Off the right side.
    if x1 > viewwidth {
        return;
    }

    tx += *spritewidth.add(lump as usize);
    let x2 = ((centerxfrac + FixedMul(tx, pspritescale)) >> FRACBITS) - 1;

    // Off the left side.
    if x2 < 0 {
        return;
    }

    // Store information in a vissprite.
    let mut avis: vissprite_t = unsafe { std::mem::zeroed() };
    let vis = &mut avis;
    vis.mobjflags = 0;
    vis.texturemid =
        (BASEYCENTER << FRACBITS) + FRACUNIT / 2 - (psp.sy - *spritetopoffset.add(lump as usize));
    vis.x1 = if x1 < 0 { 0 } else { x1 };
    vis.x2 = if x2 >= viewwidth { viewwidth - 1 } else { x2 };
    vis.scale = pspritescale << detailshift;

    if flip != 0 {
        vis.xiscale = -pspriteiscale;
        vis.startfrac = *spritewidth.add(lump as usize) - 1;
    } else {
        vis.xiscale = pspriteiscale;
        vis.startfrac = 0;
    }

    if vis.x1 > x1 {
        vis.startfrac += vis.xiscale * (vis.x1 - x1);
    }

    vis.patch = lump;

    if invisibility > 4 * 32 || invisibility & 8 != 0 {
        // Shadow draw.
        vis.colormap = ptr::null_mut();
    } else if !fixedcolormap.is_null() {
        // Fixed color.
        vis.colormap = fixedcolormap;
    } else if state.frame & FF_FULLBRIGHT != 0 {
        // Full bright.
        vis.colormap = colormaps;
    } else {
        // Local light.
        vis.colormap = *spritelights.add(MAXLIGHTSCALE - 1);
    }

    R_DrawVisSprite(data, view, draw, vis, masked, vis.x1, vis.x2);
}

// ---------------------------------------------------------------------------
// R_DrawPlayerSprites
// ---------------------------------------------------------------------------

/// Draw all active player weapon sprites (psprites) for the current frame.
///
/// Determines the lighting level from the sector the player is standing in,
/// sets `mfloorclip` and `mceilingclip` to their full-screen defaults
/// (`screenheightarray` and `negonearray` respectively), then iterates over
/// `viewplayer.psprites` calling `R_DrawPSprite` for each slot with a
/// non-null state.
///
/// Only called from `R_DrawMasked` when `viewangleoffset == 0` (i.e., the
/// player is not looking through a camera or mirror).
/// # Safety
/// - `R_SetupFrame` must have set `viewplayer`, whose `mo` must name a map
///   object of `world`'s level with a valid `subsector -> sector` chain.
/// - `scalelight`, `extralight`, `screenheightarray`, and `negonearray` must
///   all be valid before this function is called.
pub unsafe fn R_DrawPlayerSprites(world: &World, renderer: &mut Renderer) {
    let player = &world.players[renderer.view.player()];
    let mo = &world.mobjs[player.mo()];
    let lightlevel = world.level.subsector_sector(mo.subsector).lightlevel;
    let view = &renderer.view;
    renderer.sprites.spritelights = sprite_light_row(view, lightlevel, view.extralight);

    // Clip to screen bounds.
    let sprites = &mut renderer.sprites;
    let mut masked = MaskedColumnParams {
        mfloorclip: sprites.screenheightarray.as_mut_ptr(),
        mceilingclip: sprites.negonearray.as_mut_ptr(),
        spryscale: 0,
        sprtopscreen: 0,
    };

    // Add all active psprites.
    let invisibility = player.powers[pw_invisibility];
    for psp in &player.psprites {
        if !psp.state.is_null() {
            R_DrawPSprite(renderer, invisibility, psp, &mut masked);
        }
    }
}

// ---------------------------------------------------------------------------
// R_SortVisSprites
// ---------------------------------------------------------------------------

/// Sort the vissprite pool into a back-to-front doubly-linked list for the
/// painter's algorithm.
///
/// Builds an `unsorted` circular list from the pool entries, then repeatedly
/// extracts the entry with the smallest `scale` value (farthest away) and
/// appends it to `vsprsortedhead`. This is an O(n^2) selection sort, matching
/// the original C implementation exactly.
///
/// After this call, `vsprsortedhead.next` is the farthest sprite and
/// `vsprsortedhead.prev` is the closest; traversing `next` links draws
/// sprites back-to-front.
/// Called after all `R_ProjectSprite` calls for the frame and before
/// `R_DrawMasked` iterates `vsprsortedhead`. Rewrites the `next`/`prev`
/// pointers of every vissprite in the pool; the links stay valid while the
/// pool stays in place.
pub fn R_SortVisSprites(pool: &mut VisSpriteState) {
    let vissprites: *mut vissprite_t = pool.vissprites.as_mut_ptr();
    let head: *mut vissprite_t = &raw mut pool.vsprsortedhead;
    let count = pool.vissprite_p;

    let mut unsorted_node: vissprite_t = unsafe { std::mem::zeroed() };
    let unsorted: *mut vissprite_t = &raw mut unsorted_node;

    // SAFETY: `count` is at most `MAXVISSPRITES`, and every pointer
    // followed is one of the first `count` pool entries, the head or the
    // local unsorted node, all linked here.
    unsafe {
        (*unsorted).next = unsorted;
        (*unsorted).prev = unsorted;

        if count == 0 {
            return;
        }

        for i in 0..count {
            let ds = vissprites.add(i);
            (*ds).next = ds.wrapping_add(1);
            (*ds).prev = ds.wrapping_sub(1);
        }

        (*vissprites).prev = unsorted;
        (*unsorted).next = vissprites;
        (*vissprites.add(count - 1)).next = unsorted;
        (*unsorted).prev = vissprites.add(count - 1);

        // Pull the vissprites out by scale.
        (*head).next = head;
        (*head).prev = head;

        for _ in 0..count {
            let mut bestscale = i32::MAX;
            let mut best = (*unsorted).next;

            let mut ds = (*unsorted).next;
            while ds != unsorted {
                if (*ds).scale < bestscale {
                    bestscale = (*ds).scale;
                    best = ds;
                }
                ds = (*ds).next;
            }

            (*(*best).next).prev = (*best).prev;
            (*(*best).prev).next = (*best).next;
            (*best).next = head;
            (*best).prev = (*head).prev;
            (*(*head).prev).next = best;
            (*head).prev = best;
        }
    }
}

// ---------------------------------------------------------------------------
// R_DrawSprite
// ---------------------------------------------------------------------------

/// Clip and draw one sorted vissprite against the drawseg silhouette list.
///
/// Scans drawsegs from back to front to build per-column floor (`clipbot`) and
/// ceiling (`cliptop`) clip arrays. For each drawseg that overlaps the sprite's
/// horizontal span:
/// - If the seg is behind the sprite and has a masked mid-texture,
///   `R_RenderMaskedSegRange` is called immediately (mid-textures must be
///   drawn in depth order relative to sprites).
/// - If the seg is in front of the sprite, its silhouette (`SIL_BOTTOM`,
///   `SIL_TOP`, or both) is used to tighten `clipbot`/`cliptop` for those
///   columns not yet set.
///
/// After all drawsegs are processed, unset clip values default to `viewheight`
/// (bottom) and `-1` (top), then `R_DrawVisSprite` is called.
/// # Safety
/// - `spr` must be a vissprite of the sorted list.
/// - `ds_p` and `drawsegs` must be valid for the current frame, drawn from
///   `level`.
/// - `R_SortVisSprites` must have been called this frame before any calls to
///   this function.
pub unsafe fn R_DrawSprite(renderer: &mut Renderer, level: &LevelState, spr: &vissprite_t) {
    let viewheight = renderer.draw.viewheight;
    let ds_p = renderer.bsp.ds_p;

    {
        let pool = &mut renderer.vissprites;
        for x in spr.x1..=spr.x2 {
            pool.clipbot[x as usize] = -2;
            pool.cliptop[x as usize] = -2;
        }

        // Scan drawsegs from end to start for obscuring segs.
        for index in (0..ds_p).rev() {
            let ds = renderer.bsp.drawsegs[index];

            // Determine if the drawseg obscures the sprite.
            if ds.x1 > spr.x2
                || ds.x2 < spr.x1
                || (ds.silhouette == 0 && ds.maskedtexturecol.is_null())
            {
                // Does not cover sprite.
                continue;
            }

            let r1 = if ds.x1 < spr.x1 { spr.x1 } else { ds.x1 };
            let r2 = if ds.x2 > spr.x2 { spr.x2 } else { ds.x2 };

            let (lowscale, scale): (fixed_t, fixed_t);
            if ds.scale1 > ds.scale2 {
                lowscale = ds.scale2;
                scale = ds.scale1;
            } else {
                lowscale = ds.scale1;
                scale = ds.scale2;
            }

            if scale < spr.scale
                || (lowscale < spr.scale
                    && R_PointOnSegSide(level, spr.gx, spr.gy, ds.curline) == 0)
            {
                // Masked mid texture?
                if !ds.maskedtexturecol.is_null() {
                    R_RenderMaskedSegRange(renderer, level, &ds, r1, r2);
                }
                // Seg is behind sprite.
                continue;
            }

            // Clip this piece of the sprite.
            let pool = &mut renderer.vissprites;
            let (clipbot, cliptop) = (&mut pool.clipbot, &mut pool.cliptop);
            let mut silhouette = ds.silhouette;

            if spr.gz >= ds.bsilheight {
                silhouette &= !SIL_BOTTOM;
            }
            if spr.gzt <= ds.tsilheight {
                silhouette &= !SIL_TOP;
            }

            if silhouette == SIL_BOTTOM {
                for x in r1..=r2 {
                    if clipbot[x as usize] == -2 {
                        clipbot[x as usize] = *ds.sprbottomclip.add(x as usize);
                    }
                }
            } else if silhouette == SIL_TOP {
                for x in r1..=r2 {
                    if cliptop[x as usize] == -2 {
                        cliptop[x as usize] = *ds.sprtopclip.add(x as usize);
                    }
                }
            } else if silhouette == (SIL_TOP | SIL_BOTTOM) {
                for x in r1..=r2 {
                    if clipbot[x as usize] == -2 {
                        clipbot[x as usize] = *ds.sprbottomclip.add(x as usize);
                    }
                    if cliptop[x as usize] == -2 {
                        cliptop[x as usize] = *ds.sprtopclip.add(x as usize);
                    }
                }
            }
        }

        // All clipping has been performed, so draw the sprite.
        // Check for unclipped columns.
        let pool = &mut renderer.vissprites;
        for x in spr.x1..=spr.x2 {
            if pool.clipbot[x as usize] == -2 {
                pool.clipbot[x as usize] = viewheight as i16;
            }
            if pool.cliptop[x as usize] == -2 {
                pool.cliptop[x as usize] = -1;
            }
        }
    }

    let Renderer {
        data,
        view,
        draw,
        vissprites,
        ..
    } = renderer;
    let mut masked = MaskedColumnParams {
        mfloorclip: vissprites.clipbot.as_mut_ptr(),
        mceilingclip: vissprites.cliptop.as_mut_ptr(),
        spryscale: 0,
        sprtopscreen: 0,
    };
    R_DrawVisSprite(data, view, draw, spr, &mut masked, spr.x1, spr.x2);
}

// ---------------------------------------------------------------------------
// R_DrawMasked
// ---------------------------------------------------------------------------

/// The masked phase, the last of each frame.
///
/// Performs four steps in order:
/// 1. Sorts the vissprite pool back-to-front via `R_SortVisSprites`.
/// 2. Draws each sorted vissprite with `R_DrawSprite` (which also interleaves
///    any masked mid-textures that are depth-behind the sprite).
/// 3. Draws any remaining masked mid-textures that were not rendered in step 2.
/// 4. Draws the player weapon overlay via `R_DrawPlayerSprites` (skipped when
///    `viewangleoffset != 0`, i.e., in demo playback camera modes).
///
/// # Safety
/// Must be called after the BSP phase has filled the drawseg list and the
/// vissprite pool for the current frame from `world`. All renderer state
/// (`ds_p`, `drawsegs`, `viewangleoffset`, etc.) must be valid.
pub unsafe fn R_DrawMasked(world: &World, renderer: &mut Renderer) {
    let level = &world.level;
    R_SortVisSprites(&mut renderer.vissprites);

    if renderer.vissprites.vissprite_p > 0 {
        // Draw all vissprites back to front, following the links the sort
        // made; drawing a sprite leaves them alone.
        let head: *mut vissprite_t = &raw mut renderer.vissprites.vsprsortedhead;
        let mut spr = (*head).next;
        while spr != head {
            let sprite = *spr;
            R_DrawSprite(renderer, level, &sprite);
            spr = sprite.next;
        }
    }

    // Render any remaining masked mid textures.
    let ds_p = renderer.bsp.ds_p;
    for index in (0..ds_p).rev() {
        let ds = renderer.bsp.drawsegs[index];
        if !ds.maskedtexturecol.is_null() {
            R_RenderMaskedSegRange(renderer, level, &ds, ds.x1, ds.x2);
        }
    }

    // Draw the psprites on top of everything.
    if renderer.view.viewangleoffset == 0 {
        R_DrawPlayerSprites(world, renderer);
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::engine::map::sector_t};

    /// The sprite pass stamps each sector it visits with the frame's
    /// `validcount` handed down by the BSP traversal, so a sector split
    /// into several subsectors adds its things once; other sectors keep
    /// their stamp.
    #[test]
    fn add_sprites_stamps_the_sector_with_the_frame_validcount() {
        let mut level = LevelState {
            numsectors: 2,
            sectors: vec![sector_t::ZERO; 2],
            ..LevelState::new()
        };
        let mut renderer = Box::new(Renderer::new());
        // SAFETY: the sectors have no things to project.
        unsafe {
            R_AddSprites(
                &mut renderer,
                &mut level,
                &Arena::new(),
                7,
                SectorId::new(1),
            );
            R_AddSprites(
                &mut renderer,
                &mut level,
                &Arena::new(),
                7,
                SectorId::new(1),
            );
        }
        assert_eq!(level.sectors[1].validcount, 7);
        assert_eq!(level.sectors[0].validcount, 0);
    }

    /// `R_SortVisSprites` links the pool back to front (smallest scale
    /// first), keeping pool order between equal scales.
    #[test]
    fn sort_vissprites_orders_by_scale() {
        let mut pool = Box::new(VisSpriteState::new());
        R_ClearSprites(&mut pool);
        for scale in [30, 10, 20, 10] {
            R_NewVisSprite(&mut pool).scale = scale;
        }
        R_SortVisSprites(&mut pool);

        let base: *const vissprite_t = pool.vissprites.as_ptr();
        let head: *const vissprite_t = &raw const pool.vsprsortedhead;
        let mut order = Vec::new();
        // SAFETY: the sort linked the head and the four pool entries.
        unsafe {
            let mut spr: *const vissprite_t = (*head).next;
            while spr != head {
                order.push(spr.offset_from(base));
                spr = (*spr).next;
            }
        }
        assert_eq!(order, [1, 3, 2, 0]);
    }

    /// The sprite state starts with no sprite table and zeroed clip arrays.
    #[test]
    fn state_starts_empty() {
        let sprite_state = SpriteState::new();
        assert!(sprite_state.sprites.is_null() && sprite_state.spritelights.is_null());
        assert_eq!((sprite_state.numsprites, sprite_state.maxframe), (0, 0));
        assert!(sprite_state.spritename.is_empty());
        assert!(sprite_state.negonearray.iter().all(|&v| v == 0));
        assert_eq!(VisSpriteState::new().vissprite_p, 0);
    }
}
