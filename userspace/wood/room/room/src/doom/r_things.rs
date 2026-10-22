//! Sprite rendering for the Doom software renderer, ported from `vendor/doomgeneric/r_things.c`.
//!
//! Handles projection of map objects (things) into screen-space `vissprite_t` records,
//! depth-sorting them back-to-front (painter's algorithm), clipping each sprite against
//! floor/ceiling silhouettes from the BSP drawseg list, and drawing masked columns for
//! both world sprites and player weapon (psprite) overlays.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::i_error;
use std::ffi::{c_char, c_int, c_short, c_void, CStr};
use std::ptr;

use crate::doom::c_ffi::{spriteframe_t, vissprite_t, BASEYCENTER, MINZ};
use crate::doom::d_player::{PspdefT, NUMPSPRITES};
use crate::doom::info::*;
use crate::doom::m_fixed::{fixed_t, FixedDiv, FixedMul};
use crate::doom::m_fixed::{FRACBITS, FRACUNIT};
use crate::doom::r_bsp::sector_t;
use crate::doom::tables::ANG45;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Screen width in pixels; mirrors `SCREENWIDTH` from `i_video`.
const SCREENWIDTH: usize = crate::doom::i_video::SCREENWIDTH as usize;

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
const FF_FRAMEMASK: c_int = 0x7fff;

/// Flag bit in a state frame field indicating the sprite should be drawn
/// at full brightness regardless of sector lighting.
const FF_FULLBRIGHT: c_int = 0x8000;

/// Index into the player `powers` array for the partial-invisibility power-up.
/// Used when deciding whether to draw the player weapon with a fuzz (shadow)
/// column function.
const pw_invisibility: usize = 2;

/// Silhouette flag: the drawseg has a valid bottom silhouette that can clip
/// sprites from below.
const SIL_BOTTOM: c_int = 1;

/// Silhouette flag: the drawseg has a valid top silhouette that can clip
/// sprites from above.
const SIL_TOP: c_int = 2;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Sprite definition table entry for one sprite name (e.g., "TROO").
/// Points to an array of `spriteframe_t` records, one per animation frame.
/// Mirrors `spritedef_t` from `r_local.h`.
#[repr(C)]
#[derive(Clone, Copy)]
struct spritedef_t {
    /// Number of animation frames defined for this sprite.
    numframes: c_int,
    /// Pointer to the heap-allocated array of frame descriptors.
    spriteframes: *mut spriteframe_t,
}

/// On-disk / in-memory header of a `patch_t` graphic lump.
/// The column offset array immediately follows in the lump data (not part of
/// this struct), accessed via pointer arithmetic in `R_DrawVisSprite`.
/// Packed to match the WAD lump layout exactly.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct patch_t {
    /// Total width of the patch in pixels.
    width: i16,
    /// Total height of the patch in pixels.
    height: i16,
    /// Horizontal draw offset from the patch origin to column 0.
    leftoffset: i16,
    /// Vertical draw offset from the patch origin to row 0.
    topoffset: i16,
}

/// Header of a single vertical post (run of opaque pixels) within a `patch_t`
/// column. The actual pixel bytes follow immediately after this struct in memory.
/// A `topdelta` value of `0xff` signals the end of the column.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct column_t {
    /// Y offset of the top of this post relative to the patch top.
    topdelta: u8,
    /// Number of pixel rows in this post.
    length: u8,
}

// ---------------------------------------------------------------------------
// Externs from other modules
// ---------------------------------------------------------------------------

extern "C" {
    /// Case-insensitive string comparison of at most `n` bytes.
    /// Used to match 4-character sprite names from WAD lump headers.
    fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
}

use crate::doom::doomstat::modifiedgame;
use crate::doom::r_bsp::{drawsegs, ds_p};
use crate::doom::r_data::{
    colormaps, firstspritelump, lastspritelump, spriteoffset, spritetopoffset, spritewidth,
};
use crate::doom::r_draw::{
    dc_colormap, dc_iscale, dc_source, dc_texturemid, dc_translation, dc_x, dc_yh, dc_yl,
    viewheight, viewwidth,
};
use crate::doom::r_main::{
    basecolfunc, centerxfrac, centeryfrac, colfunc, detailshift, extralight, fixedcolormap,
    fuzzcolfunc, projection, scalelight, transcolfunc, validcount, viewangleoffset, viewcos,
    viewplayer, viewsin, viewx, viewy, viewz, R_PointOnSegSide, R_PointToAngle,
};
use crate::doom::r_segs::R_RenderMaskedSegRange;
use crate::doom::w_wad::{lumpinfo, W_CacheLumpNum, W_GetNumForName};
use crate::doom::z_zone::Z_Malloc;

// ---------------------------------------------------------------------------
// Globals defined by this module
// ---------------------------------------------------------------------------

/// Scale applied to player weapon (psprite) columns this frame.
#[no_mangle]
pub static mut pspritescale: fixed_t = 0;

/// Inverse of pspritescale.
#[no_mangle]
pub static mut pspriteiscale: fixed_t = 0;

/// Pointer to the active light-table array for sprites this frame.
#[no_mangle]
pub static mut spritelights: *mut *mut u8 = ptr::null_mut();

/// Clipping array initialised to -1 for psprite bottom clipping.
#[no_mangle]
pub static mut negonearray: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];

/// Clipping array initialised to viewheight for psprite top clipping.
#[no_mangle]
pub static mut screenheightarray: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];

/// Pointer to the sprite definition table.
#[no_mangle]
pub static mut sprites: *mut c_void = ptr::null_mut();

/// Total number of sprite names found in the WAD.
#[no_mangle]
pub static mut numsprites: c_int = 0;

/// Temporary frame-building array used during R_InitSprites.
#[no_mangle]
pub static mut sprtemp: [spriteframe_t; 29] = [spriteframe_t {
    rotate: 0,
    lump: [0; 8],
    flip: [0; 8],
}; 29];

/// Highest frame index seen for the current sprite during R_InitSprites.
#[no_mangle]
pub static mut maxframe: c_int = 0;

/// Name of the sprite currently being processed by R_InitSprites.
#[no_mangle]
pub static mut spritename: *mut c_char = ptr::null_mut();

// ---------------------------------------------------------------------------
// Module-local state
// ---------------------------------------------------------------------------

/// Fixed-size pool of projected sprite records for the current frame.
/// Accessed sequentially via `vissprite_p`; up to `MAXVISSPRITES` entries.
static mut vissprites: [vissprite_t; MAXVISSPRITES] = unsafe { std::mem::zeroed() };

// ---------------------------------------------------------------------------
// Module-local state (also referenced by r_segs.rs via extern "C")
// ---------------------------------------------------------------------------

/// Pointer to the next free slot in `vissprites`; advanced by `R_NewVisSprite`.
static mut vissprite_p: *mut vissprite_t = ptr::null_mut();

/// Unused counter retained for ABI compatibility with the C original.
static mut newvissprite: c_int = 0;

/// Sentinel vissprite returned when the pool is exhausted so callers always
/// receive a valid (though discarded) write target.
static mut overflowsprite: vissprite_t = unsafe { std::mem::zeroed() };

/// Mutable pointers used by masked column drawing (read by r_segs.rs).
#[no_mangle]
pub static mut mfloorclip: *mut c_short = ptr::null_mut();

/// Mutable pointers used by masked column drawing (read by r_segs.rs).
#[no_mangle]
pub static mut mceilingclip: *mut c_short = ptr::null_mut();

/// Current sprite Y scale (read by r_segs.rs).
#[no_mangle]
pub static mut spryscale: fixed_t = 0;

/// Screen Y coordinate of sprite top (read by r_segs.rs).
#[no_mangle]
pub static mut sprtopscreen: fixed_t = 0;

/// Dummy head node of the doubly-linked sorted vissprite list built by
/// `R_SortVisSprites`; iterated by `R_DrawMasked`.
static mut vsprsortedhead: vissprite_t = unsafe { std::mem::zeroed() };

// ---------------------------------------------------------------------------
// R_InstallSpriteLump
// ---------------------------------------------------------------------------

/// Record one WAD lump as a rotation of a sprite frame in `sprtemp`.
///
/// Called exclusively from `R_InitSpriteDefs` for each lump whose name
/// matches the current sprite being processed. If `rotation == 0` the lump
/// is installed for all eight rotations; otherwise it fills exactly one slot.
/// Calls `i_error!` on duplicate or inconsistent assignments.
///
/// # Safety
/// `spritename` must be a valid, null-terminated C string pointer.
/// `lump`, `frame`, and `rotation` must be within the ranges enforced by
/// the guards at the top of the function.
unsafe fn R_InstallSpriteLump(lump: c_int, frame: u32, rotation: u32, flipped: c_int) {
    if frame >= 29 || rotation > 8 {
        i_error!("R_InstallSpriteLump: Bad frame characters in lump {}", lump);
    }

    if frame as c_int > maxframe {
        maxframe = frame as c_int;
    }

    if rotation == 0 {
        // The lump should be used for all rotations.
        if sprtemp[frame as usize].rotate == 0 {
            i_error!(
                "R_InitSprites: Sprite {} frame {} has multip rot=0 lump",
                CStr::from_ptr(spritename).to_string_lossy(),
                char::from(b'A' + frame as u8)
            );
        }
        if sprtemp[frame as usize].rotate == 1 {
            i_error!(
                "R_InitSprites: Sprite {} frame {} has rotations and a rot=0 lump",
                CStr::from_ptr(spritename).to_string_lossy(),
                char::from(b'A' + frame as u8)
            );
        }
        sprtemp[frame as usize].rotate = 0;
        for r in 0..8 {
            sprtemp[frame as usize].lump[r] = (lump - firstspritelump) as c_short;
            sprtemp[frame as usize].flip[r] = flipped as u8;
        }
        return;
    }

    // The lump is only used for one rotation.
    if sprtemp[frame as usize].rotate == 0 {
        i_error!(
            "R_InitSprites: Sprite {} frame {} has rotations and a rot=0 lump",
            CStr::from_ptr(spritename).to_string_lossy(),
            char::from(b'A' + frame as u8)
        );
    }

    sprtemp[frame as usize].rotate = 1;

    // Make 0 based.
    let rot = (rotation - 1) as usize;
    if sprtemp[frame as usize].lump[rot] != -1 {
        i_error!(
            "R_InitSprites: Sprite {} : {} : {} has two lumps mapped to it",
            CStr::from_ptr(spritename).to_string_lossy(),
            char::from(b'A' + frame as u8),
            char::from(b'1' + rot as u8)
        );
    }

    sprtemp[frame as usize].lump[rot] = (lump - firstspritelump) as c_short;
    sprtemp[frame as usize].flip[rot] = flipped as u8;
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
/// `namelist` must be a null-terminated array of valid C string pointers.
/// `firstspritelump` and `lastspritelump` from `r_data` must already be
/// initialised before this function is called.
unsafe fn R_InitSpriteDefs(namelist: *mut *mut c_char) {
    let mut check = namelist;
    while !(*check).is_null() {
        check = check.add(1);
    }

    numsprites = check.offset_from(namelist) as c_int;

    if numsprites == 0 {
        return;
    }

    sprites = Z_Malloc(
        (numsprites as usize * std::mem::size_of::<spritedef_t>()) as c_int,
        1, // PU_STATIC
        ptr::null_mut(),
    );

    let start = firstspritelump - 1;
    let end = lastspritelump + 1;

    for i in 0..numsprites {
        spritename = *namelist.add(i as usize);
        ptr::write_bytes(std::ptr::addr_of_mut!(sprtemp[0]), 0xFF, 29);

        maxframe = -1;

        // Scan the lumps, filling in the frames for whatever is found.
        for l in (start + 1)..end {
            let li = &*lumpinfo.add(l as usize);
            if strncasecmp(li.name.as_ptr(), spritename, 4) == 0 {
                let frame = (li.name[4] as u8 - b'A') as u32;
                let rotation = (li.name[5] as u8 - b'0') as u32;

                let patched = if modifiedgame.is_truthy() {
                    // Need a null-terminated copy for W_GetNumForName
                    let mut name_buf: [c_char; 9] = [0; 9];
                    ptr::copy_nonoverlapping(li.name.as_ptr(), name_buf.as_mut_ptr(), 8);
                    W_GetNumForName(name_buf.as_mut_ptr())
                } else {
                    l
                };

                R_InstallSpriteLump(patched, frame, rotation, 0);

                if li.name[6] != 0 {
                    let frame = (li.name[6] as u8 - b'A') as u32;
                    let rotation = (li.name[7] as u8 - b'0') as u32;
                    R_InstallSpriteLump(l, frame, rotation, 1);
                }
            }
        }

        // Check the frames that were found for completeness.
        if maxframe == -1 {
            let spr = &mut *(sprites as *mut spritedef_t).add(i as usize);
            spr.numframes = 0;
            continue;
        }

        maxframe += 1;

        for frame in 0..maxframe {
            match sprtemp[frame as usize].rotate {
                -1 => {
                    // No rotations were found for that frame at all.
                    i_error!(
                        "R_InitSprites: No patches found for {} frame {}",
                        CStr::from_ptr(spritename).to_string_lossy(),
                        char::from(b'A' + frame as u8)
                    );
                }
                0 => {
                    // Only the first rotation is needed.
                }
                1 => {
                    // Must have all 8 frames.
                    for rotation in 0..8 {
                        if sprtemp[frame as usize].lump[rotation] == -1 {
                            i_error!(
                                "R_InitSprites: Sprite {} frame {} is missing rotations",
                                CStr::from_ptr(spritename).to_string_lossy(),
                                char::from(b'A' + frame as u8)
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        // Allocate space for the frames present and copy sprtemp to it.
        let spr = &mut *(sprites as *mut spritedef_t).add(i as usize);
        spr.numframes = maxframe;
        spr.spriteframes = Z_Malloc(
            (maxframe as usize * std::mem::size_of::<spriteframe_t>()) as c_int,
            1, // PU_STATIC
            ptr::null_mut(),
        ) as *mut spriteframe_t;
        ptr::copy_nonoverlapping(
            std::ptr::addr_of!(sprtemp[0]),
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
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// `namelist` must be a null-terminated array of valid C string pointers, each
/// pointing to a 4-character sprite name. Must be called after WAD loading
/// (`W_InitMultipleFiles`) has set up `firstspritelump` and `lastspritelump`.
#[no_mangle]
pub unsafe extern "C" fn R_InitSprites(namelist: *mut *mut c_char) {
    for i in 0..SCREENWIDTH {
        negonearray[i] = -1;
    }
    R_InitSpriteDefs(namelist);
}

// ---------------------------------------------------------------------------
// R_ClearSprites
// ---------------------------------------------------------------------------

/// Reset the vissprite pool to empty at the start of each frame.
///
/// Resets `vissprite_p` to the beginning of the `vissprites` array so that
/// the next frame can overwrite all previous entries.
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// Must be called once per frame before any call to `R_AddSprites` or
/// `R_ProjectSprite`. No other thread may access `vissprites` concurrently.
#[no_mangle]
pub unsafe extern "C" fn R_ClearSprites() {
    vissprite_p = std::ptr::addr_of_mut!(vissprites[0]);
}

// ---------------------------------------------------------------------------
// R_NewVisSprite
// ---------------------------------------------------------------------------

/// Allocate the next vissprite slot from the pool and return a pointer to it.
///
/// If the pool is full (`MAXVISSPRITES` entries already allocated this frame)
/// returns a pointer to `overflowsprite` so the caller can write without
/// crashing; the overflow record is silently discarded.
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// Must only be called after `R_ClearSprites` has been called this frame.
/// The returned pointer is valid until the next `R_ClearSprites` call.
#[no_mangle]
pub unsafe extern "C" fn R_NewVisSprite() -> *mut vissprite_t {
    if vissprite_p == std::ptr::addr_of_mut!(vissprites[0]).add(MAXVISSPRITES) {
        return &raw mut overflowsprite;
    }
    vissprite_p = vissprite_p.add(1);
    vissprite_p.sub(1)
}

// ---------------------------------------------------------------------------
// R_DrawMaskedColumn
// ---------------------------------------------------------------------------

/// Draw one vertical column of a masked (partly transparent) sprite or
/// mid-texture using the current `colfunc`.
///
/// Iterates over the post list stored in `column`, clipping each post against
/// `mfloorclip` and `mceilingclip` before invoking `colfunc`. Used for both
/// world sprites (via `R_DrawVisSprite`) and masked mid-textures
/// (via `R_RenderMaskedSegRange`).
/// Exported as `#[no_mangle]` for C callers (called from `r_segs.rs`).
///
/// # Safety
/// - `column` must point to a valid `column_t` sequence terminated by
///   `topdelta == 0xff`.
/// - `mfloorclip`, `mceilingclip`, `dc_x`, `sprtopscreen`, and `spryscale`
///   must all be valid for the current column being drawn.
/// - `colfunc` must be set to a valid column renderer before calling.
#[no_mangle]
pub unsafe extern "C" fn R_DrawMaskedColumn(column: *mut c_void) {
    let mut column = column as *mut column_t;
    let basetexturemid = dc_texturemid;

    while (*column).topdelta != 0xff {
        // Calculate unclipped screen coordinates for post.
        let topscreen = sprtopscreen + spryscale * (*column).topdelta as c_int;
        let bottomscreen = topscreen + spryscale * (*column).length as c_int;

        dc_yl = (topscreen + FRACUNIT - 1) >> FRACBITS;
        dc_yh = (bottomscreen - 1) >> FRACBITS;

        if dc_yh >= *mfloorclip.add(dc_x as usize) as c_int {
            dc_yh = *mfloorclip.add(dc_x as usize) as c_int - 1;
        }
        if dc_yl <= *mceilingclip.add(dc_x as usize) as c_int {
            dc_yl = *mceilingclip.add(dc_x as usize) as c_int + 1;
        }

        if dc_yl <= dc_yh {
            dc_source = (column as *mut u8).add(3);
            dc_texturemid = basetexturemid - (((*column).topdelta as c_int) << FRACBITS);

            if let Some(func) = colfunc {
                func();
            }
        }
        column = (column as *mut u8).add((*column).length as usize + 4) as *mut column_t;
    }

    dc_texturemid = basetexturemid;
}

// ---------------------------------------------------------------------------
// R_DrawVisSprite
// ---------------------------------------------------------------------------

/// Draw a fully projected vissprite to the screen.
///
/// Sets up the column renderer state (`dc_colormap`, `dc_iscale`,
/// `dc_texturemid`, `spryscale`, `sprtopscreen`) from the vissprite fields,
/// then iterates over each screen column from `vis.x1` to `vis.x2`, locating
/// the corresponding patch column and calling `R_DrawMaskedColumn`.
///
/// The column function (`colfunc`) is temporarily overridden to `fuzzcolfunc`
/// for shadow-drawn sprites (null colormap) or `transcolfunc` for translated
/// sprites, and restored to `basecolfunc` afterwards.
///
/// `mfloorclip` and `mceilingclip` must be set by the caller before this
/// function is invoked.
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// - `vis` must point to a fully initialised `vissprite_t` as produced by
///   `R_ProjectSprite` or `R_DrawPSprite`.
/// - The patch lump referenced by `vis.patch + firstspritelump` must be loaded
///   in the WAD cache.
/// - `mfloorclip` and `mceilingclip` must be valid arrays of at least
///   `SCREENWIDTH` elements.
#[no_mangle]
pub unsafe extern "C" fn R_DrawVisSprite(vis: *mut vissprite_t, _x1: c_int, _x2: c_int) {
    let patch = W_CacheLumpNum((*vis).patch + firstspritelump, 8) as *mut patch_t; // PU_CACHE = 8

    dc_colormap = (*vis).colormap;

    if dc_colormap.is_null() {
        // NULL colormap = shadow draw.
        colfunc = fuzzcolfunc;
    } else if (*vis).mobjflags & MF_TRANSLATION != 0 {
        colfunc = transcolfunc;
        dc_translation = colormaps
            .sub(256)
            .add((((*vis).mobjflags & MF_TRANSLATION) >> (MF_TRANSSHIFT - 8)) as usize);
    }

    dc_iscale = ((*vis).xiscale.abs() >> detailshift) as c_int;
    dc_texturemid = (*vis).texturemid;
    let mut frac = (*vis).startfrac;
    spryscale = (*vis).scale;
    sprtopscreen = centeryfrac - FixedMul(dc_texturemid, spryscale);

    for x in (*vis).x1..=(*vis).x2 {
        dc_x = x;
        let texturecolumn = frac >> FRACBITS;

        #[cfg(feature = "rangecheck")]
        {
            let patch_width = (*patch).width as c_int;
            if texturecolumn < 0 || texturecolumn >= patch_width {
                i_error!("R_DrawSpriteRange: bad texturecolumn");
            }
        }

        let columnofs = (patch as *mut u8).add(8) as *mut c_int;
        let col_offset = *columnofs.add(texturecolumn as usize);
        let col = (patch as *mut u8).add(col_offset as usize) as *mut column_t;
        R_DrawMaskedColumn(col as *mut c_void);

        frac += (*vis).xiscale;
    }

    colfunc = basecolfunc;
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
/// Exported as `#[no_mangle]` for C callers (called from `R_AddSprites`).
///
/// # Safety
/// - `thing` must be a valid, non-null `mobj_t` pointer.
/// - `sprites`, `numsprites`, `spriteoffset`, `spritewidth`, and
///   `spritetopoffset` must all be initialised by `R_InitSprites` before this
///   function is called.
/// - `viewx`, `viewy`, `viewz`, `viewcos`, `viewsin`, `projection`,
///   `centerxfrac`, `viewwidth`, and `spritelights` must be valid for the
///   current frame.
#[no_mangle]
pub unsafe extern "C" fn R_ProjectSprite(thing: *mut c_void) {
    let thing = thing as *mut crate::doom::c_ffi::mobj_t;

    // Transform the origin point.
    let tr_x = (*thing).x - viewx;
    let tr_y = (*thing).y - viewy;

    let gxt = FixedMul(tr_x, viewcos);
    let gyt = -FixedMul(tr_y, viewsin);

    let tz = gxt - gyt;

    // Thing is behind view plane?
    if tz < MINZ {
        return;
    }

    let xscale = FixedDiv(projection, tz);

    let gxt = -FixedMul(tr_x, viewsin);
    let gyt = FixedMul(tr_y, viewcos);
    let tx = -(gyt + gxt);

    // Too far off the side?
    if tx.abs() > (tz << 2) {
        return;
    }

    // Decide which patch to use for sprite relative to player.
    #[cfg(feature = "rangecheck")]
    {
        if (*thing).sprite as u32 >= numsprites as u32 {
            i_error!("R_ProjectSprite: invalid sprite number {}", (*thing).sprite);
        }
    }

    let sprdef = &*(sprites as *mut spritedef_t).add((*thing).sprite as usize);
    #[cfg(feature = "rangecheck")]
    {
        if ((*thing).frame & FF_FRAMEMASK) >= sprdef.numframes {
            if sprdef.numframes == 0 {
                return; // sprite lump not present in this WAD — skip silently
            }
            i_error!(
                "R_ProjectSprite [THING]: invalid sprite frame {} : {}",
                (*thing).sprite,
                (*thing).frame
            );
        }
    }
    let sprframe = &*sprdef
        .spriteframes
        .add(((*thing).frame & FF_FRAMEMASK) as usize);

    let (lump, flip): (c_int, c_int);
    if sprframe.rotate == 0 {
        // Use single rotation for all views.
        lump = sprframe.lump[0] as c_int;
        flip = sprframe.flip[0] as c_int;
    } else {
        // Choose a different rotation based on player view.
        let ang = R_PointToAngle((*thing).x, (*thing).y);
        let rot = ((ang
            .wrapping_sub((*thing).angle)
            .wrapping_add((ANG45 / 2) * 9))
            >> 29) as usize;
        lump = sprframe.lump[rot] as c_int;
        flip = sprframe.flip[rot] as c_int;
    }

    // Calculate edges of the shape.
    let mut tx = tx - *spriteoffset.add(lump as usize);
    let x1 = (centerxfrac + FixedMul(tx, xscale)) >> FRACBITS;

    // Off the right side?
    if x1 > viewwidth {
        return;
    }

    tx += *spritewidth.add(lump as usize);
    let x2 = ((centerxfrac + FixedMul(tx, xscale)) >> FRACBITS) - 1;

    // Off the left side.
    if x2 < 0 {
        return;
    }

    // Store information in a vissprite.
    let vis = R_NewVisSprite();
    (*vis).mobjflags = (*thing).flags;
    (*vis).scale = xscale << detailshift;
    (*vis).gx = (*thing).x;
    (*vis).gy = (*thing).y;
    (*vis).gz = (*thing).z;
    (*vis).gzt = (*thing).z + *spritetopoffset.add(lump as usize);
    (*vis).texturemid = (*vis).gzt - viewz;
    (*vis).x1 = if x1 < 0 { 0 } else { x1 };
    (*vis).x2 = if x2 >= viewwidth { viewwidth - 1 } else { x2 };
    let iscale = FixedDiv(FRACUNIT, xscale);

    if flip != 0 {
        (*vis).startfrac = *spritewidth.add(lump as usize) - 1;
        (*vis).xiscale = -iscale;
    } else {
        (*vis).startfrac = 0;
        (*vis).xiscale = iscale;
    }

    if (*vis).x1 > x1 {
        (*vis).startfrac += (*vis).xiscale * ((*vis).x1 - x1);
    }
    (*vis).patch = lump;

    // Get light level.
    if (*thing).flags & MF_SHADOW != 0 {
        // Shadow draw.
        (*vis).colormap = ptr::null_mut();
    } else if !fixedcolormap.is_null() {
        // Fixed map.
        (*vis).colormap = fixedcolormap;
    } else if (*thing).frame & FF_FULLBRIGHT != 0 {
        // Full bright.
        (*vis).colormap = colormaps;
    } else {
        // Diminished light.
        let mut index = (xscale >> (LIGHTSCALESHIFT - detailshift as u32)) as usize;
        if index >= MAXLIGHTSCALE {
            index = MAXLIGHTSCALE - 1;
        }
        (*vis).colormap = *spritelights.add(index);
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
/// thing in the sector's thing list.
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// - `sec` must be a valid, non-null `sector_t` pointer.
/// - `validcount`, `scalelight`, `extralight` must be initialised for the
///   current frame before this function is called.
#[no_mangle]
pub unsafe extern "C" fn R_AddSprites(sec: *mut sector_t) {
    let sec = &*sec;

    // BSP is traversed by subsector.
    // A sector might have been split into several subsectors during BSP building.
    // Thus we check whether it's already added.
    if sec.validcount == validcount {
        return;
    }

    // Well, now it will be done.
    (*(sec as *const sector_t as *mut sector_t)).validcount = validcount;

    let lightnum = (sec.lightlevel >> LIGHTSEGSHIFT as i16) as c_int + extralight;

    if lightnum < 0 {
        spritelights = scalelight[0].as_mut_ptr();
    } else if lightnum >= LIGHTLEVELS as c_int {
        spritelights = scalelight[LIGHTLEVELS - 1].as_mut_ptr();
    } else {
        spritelights = scalelight[lightnum as usize].as_mut_ptr();
    }

    // Handle all things in sector.
    let mut thing = sec.thinglist as *mut crate::doom::c_ffi::mobj_t;
    while !thing.is_null() {
        R_ProjectSprite(thing as *mut c_void);
        thing = (*thing).snext as *mut crate::doom::c_ffi::mobj_t;
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
/// If the player carries the partial-invisibility power-up at a sufficient
/// level the weapon is drawn with `fuzzcolfunc` (shadow effect).
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// - `psp` must be a valid, non-null `PspdefT` pointer whose `state` field
///   points to a valid `State`.
/// - `sprites`, `pspritescale`, `pspriteiscale`, `spritelights`,
///   `mfloorclip`, and `mceilingclip` must all be valid for the current frame.
/// - `viewplayer` must point to a fully initialised player structure.
#[no_mangle]
pub unsafe extern "C" fn R_DrawPSprite(psp: *mut PspdefT) {
    let psp = &*psp;

    // Decide which patch to use.
    if psp.state.is_null() {
        return;
    }

    let state = psp.state as *mut State;

    #[cfg(feature = "rangecheck")]
    {
        let sprite = (*state).sprite;
        if sprite as u32 >= numsprites as u32 {
            i_error!("R_ProjectSprite: invalid sprite number {}", sprite);
        }
    }

    let sprdef = &*(sprites as *mut spritedef_t).add((*state).sprite as usize);
    #[cfg(feature = "rangecheck")]
    {
        if ((*state).frame & FF_FRAMEMASK) >= sprdef.numframes {
            i_error!(
                "R_ProjectSprite [PSPRITE]: invalid sprite frame {} : {}",
                (*state).sprite,
                (*state).frame
            );
        }
    }
    let sprframe = &*sprdef
        .spriteframes
        .add(((*state).frame & FF_FRAMEMASK) as usize);

    let lump = sprframe.lump[0] as c_int;
    let flip = sprframe.flip[0] as c_int;

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

    let player = &*viewplayer;
    if player.powers[pw_invisibility] > 4 * 32 || player.powers[pw_invisibility] & 8 != 0 {
        // Shadow draw.
        vis.colormap = ptr::null_mut();
    } else if !fixedcolormap.is_null() {
        // Fixed color.
        vis.colormap = fixedcolormap;
    } else if (*state).frame & FF_FULLBRIGHT != 0 {
        // Full bright.
        vis.colormap = colormaps;
    } else {
        // Local light.
        vis.colormap = *spritelights.add(MAXLIGHTSCALE - 1);
    }

    R_DrawVisSprite(vis, vis.x1, vis.x2);
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
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// - `viewplayer` must point to a fully initialised player structure with a
///   valid `mo -> subsector -> sector` chain.
/// - `scalelight`, `extralight`, `screenheightarray`, and `negonearray` must
///   all be valid before this function is called.
#[no_mangle]
pub unsafe extern "C" fn R_DrawPlayerSprites() {
    let mo = (*viewplayer).mo as *mut crate::doom::c_ffi::mobj_t;
    let sub = (*mo).subsector as *mut crate::doom::r_bsp::subsector_t;
    let sec = (*sub).sector;
    let lightnum = ((*sec).lightlevel >> LIGHTSEGSHIFT as i16) as c_int + extralight;

    if lightnum < 0 {
        spritelights = scalelight[0].as_mut_ptr();
    } else if lightnum >= LIGHTLEVELS as c_int {
        spritelights = scalelight[LIGHTLEVELS - 1].as_mut_ptr();
    } else {
        spritelights = scalelight[lightnum as usize].as_mut_ptr();
    }

    // Clip to screen bounds.
    mfloorclip = std::ptr::addr_of_mut!(screenheightarray[0]);
    mceilingclip = std::ptr::addr_of_mut!(negonearray[0]);

    // Add all active psprites.
    let psp = (*viewplayer).psprites.as_ptr() as *mut PspdefT;
    for i in 0..NUMPSPRITES {
        let psp_i = psp.add(i);
        if !(*psp_i).state.is_null() {
            R_DrawPSprite(psp_i);
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
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// Must be called after all `R_ProjectSprite` calls for the frame and before
/// `R_DrawMasked` iterates `vsprsortedhead`. Modifies the `next`/`prev`
/// pointers of every vissprite in the pool.
#[no_mangle]
pub unsafe extern "C" fn R_SortVisSprites() {
    let count = vissprite_p.offset_from(std::ptr::addr_of_mut!(vissprites[0]));

    let mut unsorted: vissprite_t = unsafe { std::mem::zeroed() };
    unsorted.next = &mut unsorted;
    unsorted.prev = &mut unsorted;

    if count == 0 {
        return;
    }

    for i in 0..count {
        let ds = std::ptr::addr_of_mut!(vissprites[0]).add(i as usize);
        (*ds).next = ds.add(1);
        (*ds).prev = ds.sub(1);
    }

    vissprites[0].prev = &mut unsorted;
    unsorted.next = std::ptr::addr_of_mut!(vissprites[0]);
    (*vissprite_p.sub(1)).next = &mut unsorted;
    unsorted.prev = vissprite_p.sub(1);

    // Pull the vissprites out by scale.
    vsprsortedhead.next = &raw mut vsprsortedhead;
    vsprsortedhead.prev = &raw mut vsprsortedhead;

    for _ in 0..count {
        let mut bestscale = c_int::MAX;
        let mut best = unsorted.next;

        let mut ds = unsorted.next;
        while ds != &mut unsorted {
            if (*ds).scale < bestscale {
                bestscale = (*ds).scale;
                best = ds;
            }
            ds = (*ds).next;
        }

        (*(*best).next).prev = (*best).prev;
        (*(*best).prev).next = (*best).next;
        (*best).next = &raw mut vsprsortedhead;
        (*best).prev = vsprsortedhead.prev;
        (*vsprsortedhead.prev).next = best;
        vsprsortedhead.prev = best;
    }
}

// ---------------------------------------------------------------------------
// R_DrawSprite
// ---------------------------------------------------------------------------

/// Clip and draw one sorted vissprite against the drawseg silhouette list.
///
/// Scans drawsegs from back to front to build per-column floor (`CLIPBOT`) and
/// ceiling (`CLIPTOP`) clip arrays. For each drawseg that overlaps the sprite's
/// horizontal span:
/// - If the seg is behind the sprite and has a masked mid-texture,
///   `R_RenderMaskedSegRange` is called immediately (mid-textures must be
///   drawn in depth order relative to sprites).
/// - If the seg is in front of the sprite, its silhouette (`SIL_BOTTOM`,
///   `SIL_TOP`, or both) is used to tighten `CLIPBOT`/`CLIPTOP` for those
///   columns not yet set.
///
/// After all drawsegs are processed, unset clip values default to `viewheight`
/// (bottom) and `-1` (top), then `R_DrawVisSprite` is called.
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// - `spr` must be a valid `vissprite_t` pointer from the sorted list.
/// - `ds_p` and `drawsegs` must be valid for the current frame.
/// - `R_SortVisSprites` must have been called this frame before any calls to
///   this function.
#[no_mangle]
pub unsafe extern "C" fn R_DrawSprite(spr: *mut vissprite_t) {
    let spr = &*spr;

    static mut CLIPBOT: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];
    static mut CLIPTOP: [c_short; SCREENWIDTH] = [0; SCREENWIDTH];

    for x in spr.x1..=spr.x2 {
        CLIPBOT[x as usize] = -2;
        CLIPTOP[x as usize] = -2;
    }

    // Scan drawsegs from end to start for obscuring segs.
    let mut ds = ds_p.sub(1);
    let drawsegs_base = std::ptr::addr_of_mut!(drawsegs[0]);
    while ds >= drawsegs_base {
        // Determine if the drawseg obscures the sprite.
        if (*ds).x1 > spr.x2
            || (*ds).x2 < spr.x1
            || ((*ds).silhouette == 0 && (*ds).maskedtexturecol.is_null())
        {
            // Does not cover sprite.
            ds = ds.sub(1);
            continue;
        }

        let r1 = if (*ds).x1 < spr.x1 { spr.x1 } else { (*ds).x1 };
        let r2 = if (*ds).x2 > spr.x2 { spr.x2 } else { (*ds).x2 };

        let (lowscale, scale): (fixed_t, fixed_t);
        if (*ds).scale1 > (*ds).scale2 {
            lowscale = (*ds).scale2;
            scale = (*ds).scale1;
        } else {
            lowscale = (*ds).scale1;
            scale = (*ds).scale2;
        }

        if scale < spr.scale
            || (lowscale < spr.scale && R_PointOnSegSide(spr.gx, spr.gy, (*ds).curline) == 0)
        {
            // Masked mid texture?
            if !(*ds).maskedtexturecol.is_null() {
                R_RenderMaskedSegRange(ds, r1, r2);
            }
            // Seg is behind sprite.
            ds = ds.sub(1);
            continue;
        }

        // Clip this piece of the sprite.
        let mut silhouette = (*ds).silhouette;

        if spr.gz >= (*ds).bsilheight {
            silhouette &= !SIL_BOTTOM;
        }
        if spr.gzt <= (*ds).tsilheight {
            silhouette &= !SIL_TOP;
        }

        if silhouette == SIL_BOTTOM {
            for x in r1..=r2 {
                if CLIPBOT[x as usize] == -2 {
                    CLIPBOT[x as usize] = *(*ds).sprbottomclip.add(x as usize);
                }
            }
        } else if silhouette == SIL_TOP {
            for x in r1..=r2 {
                if CLIPTOP[x as usize] == -2 {
                    CLIPTOP[x as usize] = *(*ds).sprtopclip.add(x as usize);
                }
            }
        } else if silhouette == (SIL_TOP | SIL_BOTTOM) {
            for x in r1..=r2 {
                if CLIPBOT[x as usize] == -2 {
                    CLIPBOT[x as usize] = *(*ds).sprbottomclip.add(x as usize);
                }
                if CLIPTOP[x as usize] == -2 {
                    CLIPTOP[x as usize] = *(*ds).sprtopclip.add(x as usize);
                }
            }
        }

        ds = ds.sub(1);
    }

    // All clipping has been performed, so draw the sprite.
    // Check for unclipped columns.
    for x in spr.x1..=spr.x2 {
        if CLIPBOT[x as usize] == -2 {
            CLIPBOT[x as usize] = viewheight as c_short;
        }
        if CLIPTOP[x as usize] == -2 {
            CLIPTOP[x as usize] = -1;
        }
    }

    mfloorclip = std::ptr::addr_of_mut!(CLIPBOT[0]);
    mceilingclip = std::ptr::addr_of_mut!(CLIPTOP[0]);
    R_DrawVisSprite(
        spr as *const vissprite_t as *mut vissprite_t,
        spr.x1,
        spr.x2,
    );
}

// ---------------------------------------------------------------------------
// R_DrawMasked
// ---------------------------------------------------------------------------

/// Top-level masked rendering pass called at the end of each frame.
///
/// Performs four steps in order:
/// 1. Sorts the vissprite pool back-to-front via `R_SortVisSprites`.
/// 2. Draws each sorted vissprite with `R_DrawSprite` (which also interleaves
///    any masked mid-textures that are depth-behind the sprite).
/// 3. Draws any remaining masked mid-textures that were not rendered in step 2.
/// 4. Draws the player weapon overlay via `R_DrawPlayerSprites` (skipped when
///    `viewangleoffset != 0`, i.e., in demo playback camera modes).
///
/// Exported as `#[no_mangle]` for C callers.
///
/// # Safety
/// Must be called after `R_RenderBSPNode` has filled the drawseg list and
/// the vissprite pool for the current frame. All renderer globals (`ds_p`,
/// `drawsegs`, `viewangleoffset`, etc.) must be valid.
#[no_mangle]
pub unsafe extern "C" fn R_DrawMasked() {
    R_SortVisSprites();

    if vissprite_p > std::ptr::addr_of_mut!(vissprites[0]) {
        // Draw all vissprites back to front.
        let mut spr = vsprsortedhead.next;
        while spr != &raw mut vsprsortedhead {
            R_DrawSprite(spr);
            spr = (*spr).next;
        }
    }

    // Render any remaining masked mid textures.
    let mut ds = ds_p.sub(1);
    let drawsegs_base = std::ptr::addr_of_mut!(drawsegs[0]);
    while ds >= drawsegs_base {
        if !(*ds).maskedtexturecol.is_null() {
            R_RenderMaskedSegRange(ds, (*ds).x1, (*ds).x2);
        }
        ds = ds.sub(1);
    }

    // Draw the psprites on top of everything.
    if viewangleoffset == 0 {
        R_DrawPlayerSprites();
    }
}

// ---------------------------------------------------------------------------
// Anchor
// ---------------------------------------------------------------------------

/// Prevents the linker from dead-stripping the `#[no_mangle]` functions in
/// this module by taking their addresses.
///
/// The Rust linker may eliminate `pub unsafe extern "C"` functions that are
/// never called from Rust code; this anchor ensures all exported symbols
/// remain present for C callers.
///
/// # Safety
/// This function has no preconditions and performs no operations; it only
/// takes function addresses.
#[no_mangle]
pub unsafe extern "C" fn R_Things_Link_Anchor() {
    let _ = R_InitSprites as *const () as usize;
    let _ = R_ClearSprites as *const () as usize;
    let _ = R_NewVisSprite as *const () as usize;
    let _ = R_DrawMaskedColumn as *const () as usize;
    let _ = R_DrawVisSprite as *const () as usize;
    let _ = R_ProjectSprite as *const () as usize;
    let _ = R_AddSprites as *const () as usize;
    let _ = R_DrawPSprite as *const () as usize;
    let _ = R_DrawPlayerSprites as *const () as usize;
    let _ = R_SortVisSprites as *const () as usize;
    let _ = R_DrawSprite as *const () as usize;
    let _ = R_DrawMasked as *const () as usize;
}
