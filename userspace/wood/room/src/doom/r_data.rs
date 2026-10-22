//! Rust port of `vendor/doomgeneric/r_data.c`.
//!
//! Texture, flat, and colormap data loading, caching, and lookup.
//!
//! # Doom graphics model
//!
//! Doom wall and sprite graphics are stored as vertical runs of opaque pixels
//! called *posts*.  A *column* is zero or more posts; a *patch* (or sprite) is
//! zero or more columns.  A *texture* is a rectangular surface composed of one
//! or more patches composited together.
//!
//! # Composite texture cache
//!
//! When a texture is first needed, `R_GenerateLookup` pre-computes per-column
//! metadata stored in `texturecolumnlump` and `texturecolumnofs`:
//! - If only one patch covers a column, `texturecolumnlump[tex][col]` points
//!   directly into that patch's WAD lump, and no composite is needed.
//! - If multiple patches overlap the column, `texturecolumnlump[tex][col]` is
//!   set to `-1` and the column must be composited into a heap buffer.
//!   `R_GenerateComposite` does that work on first use and caches the result.
//!
//! `R_GetColumn` is the hot path: it returns a pointer to the column data,
//! triggering composite generation as needed.
//!
//! # Globals exported to C
//!
//! Most `#[no_mangle]` statics in this module are declared `extern` in
//! `r_state.h` and consumed by multiple renderer and physics C files.
//! Exceptions: `lastflat` and `numflats` are only declared locally in
//! `p_spec.c`, not in `r_state.h`.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use crate::i_error;
use std::ffi::{c_char, c_int, c_short, c_uint, c_ushort, c_void, CStr};

use std::ptr;

use crate::doom::c_ffi::mobj_t;
use crate::doom::m_fixed::FRACBITS;
use crate::doom::z_zone::{PU_CACHE, PU_STATIC};

// ---------------------------------------------------------------------------
// Constants & helpers
// ---------------------------------------------------------------------------

/// Identity byte-swap for little-endian targets (mirrors `i_swap.h` `SHORT`).
///
/// On all currently supported targets `i16` is already little-endian, so this
/// is a no-op.  Present to keep the code structurally parallel to the C source.
#[inline]
fn SHORT(x: i16) -> i16 {
    x
}

/// Identity word-swap for little-endian targets (mirrors `i_swap.h` `LONG`).
///
/// Analogous to `SHORT`; a no-op on little-endian hosts.
#[inline]
fn LONG(x: c_int) -> c_int {
    x
}

/// Pass-through for DeHackEd string substitution (not implemented in this port).
///
/// In the upstream Chocolate Doom source `DEH_String` allows patch files to
/// override lump names at runtime.  This port does not support DeHackEd, so
/// the function returns its argument unchanged.
///
/// # Safety
///
/// `s` must be a valid, null-terminated C string for the lifetime of the call.
unsafe fn DEH_String(s: *const c_char) -> *const c_char {
    s
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// On-disk patch entry inside a `maptexture_t` WAD record (packed, C layout).
///
/// Corresponds to `mappatch_t` in `r_data.c`.  `stepdir` and `colormap` are
/// present in the WAD format but unused by the renderer.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct mappatch_t {
    /// Horizontal origin of the patch within the texture, in pixels.
    originx: i16,
    /// Vertical origin of the patch within the texture, in pixels.
    originy: i16,
    /// Index into the PNAMES patch directory.
    patch: i16,
    /// Unused animation field (always 1 in practice).
    stepdir: i16,
    /// Unused colormap field (always 0 in practice).
    colormap: i16,
}

/// On-disk texture definition as stored in TEXTURE1 / TEXTURE2 WAD lumps (packed, C layout).
///
/// Corresponds to `maptexture_t` in `r_data.c`.  The `patches` field is a
/// C flexible-array member: the struct is followed by `patchcount - 1`
/// additional `mappatch_t` entries in memory.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct maptexture_t {
    /// Texture name, up to 8 ASCII characters, NUL-padded.
    name: [c_char; 8],
    /// Non-zero if the texture has transparent holes (unused by the renderer).
    masked: c_int,
    /// Texture width in pixels.
    width: i16,
    /// Texture height in pixels.
    height: i16,
    /// Obsolete field present in the WAD format; ignored.
    obsolete: c_int,
    /// Number of `mappatch_t` entries that follow this struct in memory.
    patchcount: i16,
    /// First patch entry; additional patches follow contiguously in WAD data.
    patches: mappatch_t,
}

/// Runtime patch descriptor stored inside a `texture_t` (C layout).
///
/// Corresponds to `texpatch_t` in `r_data.c`.  `patch` is a WAD lump number
/// (resolved from the PNAMES index during `R_InitTextures`).
#[repr(C)]
#[derive(Clone, Copy)]
struct texpatch_t {
    /// Horizontal origin of the patch within the texture, in pixels.
    originx: i16,
    /// Vertical origin of the patch within the texture, in pixels.
    originy: i16,
    /// WAD lump number of the patch graphic.
    patch: c_int,
}

/// Runtime texture descriptor held in the `textures` pointer array (C layout).
///
/// Corresponds to `texture_t` in `r_data.c`.  The `patches` field is a C
/// flexible-array member: the struct is allocated with room for
/// `patchcount - 1` additional `texpatch_t` entries immediately after.
/// The `next` pointer links entries that hash to the same bucket in
/// `textures_hashtable`.
#[repr(C)]
struct texture_t {
    /// Texture name, up to 8 ASCII characters, NUL-padded.
    name: [c_char; 8],
    /// Texture width in pixels.
    width: i16,
    /// Texture height in pixels.
    height: i16,
    /// Index of this texture in the `textures` array; set by `GenerateTextureHashTable`.
    index: c_int,
    /// Next entry in the hash-table chain for this bucket, or null.
    next: *mut texture_t,
    /// Number of `texpatch_t` entries that follow this struct in memory.
    patchcount: i16,
    /// First patch descriptor; additional patches follow contiguously.
    patches: texpatch_t,
}

/// Patch graphic header, matching the WAD `patch_t` struct (packed, C layout).
///
/// Corresponds to `patch_t` in `r_local.h`.  Immediately after the four
/// header fields, `width` 32-bit column offsets follow (the `columnofs[]`
/// array), then the column data itself.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct patch_t {
    /// Width of the patch in pixels.
    pub width: i16,
    /// Height of the patch in pixels.
    pub height: i16,
    /// Horizontal draw offset from the patch origin, in pixels.
    pub leftoffset: i16,
    /// Vertical draw offset from the patch origin, in pixels.
    pub topoffset: i16,
}

/// Single run of opaque pixels within a column (a "post"), packed C layout.
///
/// Corresponds to `post_t` in `r_local.h`.  A column is a sequence of posts
/// terminated by a `topdelta` value of `0xff`.  The actual pixel data follows
/// immediately after the two-byte header (one byte of padding before the
/// pixels and one byte of padding after).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct post_t {
    /// Row offset from the top of the texture column where this post begins.
    pub topdelta: u8,
    /// Number of pixels in this post.
    pub length: u8,
}

/// Alias for `post_t`; used interchangeably in the C source as `column_t`.
pub type column_t = post_t;

/// Per-rotation frame data for one sprite animation frame (C layout).
///
/// Corresponds to `spriteframe_t` in `r_local.h`.  `lump[r]` is the WAD
/// lump offset from `firstspritelump` for rotation `r`; `flip[r]` is non-zero
/// if that rotation should be drawn mirrored.
#[repr(C)]
#[derive(Clone, Copy)]
struct spriteframe_t {
    /// Non-zero if the sprite has per-rotation variants; 0 for a single view.
    pub rotate: c_int,
    /// Lump offsets (from `firstspritelump`) for each of the 8 rotations.
    pub lump: [c_short; 8],
    /// Mirror flags for each rotation (non-zero = draw flipped).
    pub flip: [u8; 8],
}

/// Sprite definition holding all animation frames for one sprite class (C layout).
///
/// Corresponds to `spritedef_t` in `r_local.h`.  `spriteframes` points to a
/// heap-allocated array of `numframes` `spriteframe_t` entries.
#[repr(C)]
struct spritedef_t {
    /// Number of animation frames.
    pub numframes: c_int,
    /// Heap-allocated array of frame descriptors, length `numframes`.
    pub spriteframes: *mut spriteframe_t,
}

// ---------------------------------------------------------------------------
// Externs from other modules
// ---------------------------------------------------------------------------

extern "C" {
    /// POSIX case-insensitive string comparison, at most `n` bytes.
    fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
}

use crate::doom::g_game::demoplayback;
use crate::doom::i_system::I_ConsoleStdout;
use crate::doom::m_misc::M_StringCopy;
use crate::doom::p_mobj::P_MobjThinker;
use crate::doom::p_setup::{numsectors, numsides, sectors, sides};
use crate::doom::p_tick::thinkercap;
use crate::doom::r_sky::skytexture;
use crate::doom::r_things::{numsprites, sprites};
use crate::doom::w_wad::{
    lumpinfo, W_CacheLumpName, W_CacheLumpNum, W_CheckNumForName, W_GetNumForName, W_LumpLength,
    W_LumpNameHash, W_ReleaseLumpName,
};
use crate::doom::z_zone::{Z_ChangeTag2, Z_Free, Z_Malloc};

// ---------------------------------------------------------------------------
// Globals defined by this module
// ---------------------------------------------------------------------------

/// WAD lump index of the first flat (the lump after `F_START`).
///
/// Exported as `extern int firstflat` in `r_state.h`.  Used by `r_plane.c`
/// to compute lump numbers from flat indices: `lump = firstflat + flatnum`.
#[no_mangle]
pub static mut firstflat: c_int = 0;

/// WAD lump index of the last flat (the lump before `F_END`).
///
/// Not in `r_state.h`; declared locally in `p_spec.c`.  Together with
/// `firstflat` it defines the flat lump range in the WAD.
#[no_mangle]
pub static mut lastflat: c_int = 0;

/// Total number of flat lumps (`lastflat - firstflat + 1`).
///
/// Declared locally in `p_spec.c` (not in `r_state.h`) as `extern int numflats`
/// for animation bounds checking.
#[no_mangle]
pub static mut numflats: c_int = 0;

/// WAD lump index of the first patch lump (the lump after the patch namespace marker).
///
/// Initialized but not currently used by the renderer; present to mirror the C globals.
#[no_mangle]
pub static mut firstpatch: c_int = 0;

/// WAD lump index of the last patch lump.
#[no_mangle]
pub static mut lastpatch: c_int = 0;

/// Total number of patch lumps.
#[no_mangle]
pub static mut numpatches: c_int = 0;

/// WAD lump index of the first sprite lump (the lump after `S_START`).
///
/// Exported via `r_state.h`.  Used by `r_things.c` and `f_finale.c` to
/// convert sprite-relative lump indices to absolute WAD lump numbers:
/// `lump = firstspritelump + relative`.
#[no_mangle]
pub static mut firstspritelump: c_int = 0;

/// WAD lump index of the last sprite lump (the lump before `S_END`).
///
/// Exported via `r_state.h`.  Used by `r_things.c` when scanning the WAD for
/// sprite frames.
#[no_mangle]
pub static mut lastspritelump: c_int = 0;

/// Total number of sprite lumps (`lastspritelump - firstspritelump + 1`).
///
/// Exported via `r_state.h`.  Bounds the `spritewidth`, `spriteoffset`, and
/// `spritetopoffset` arrays.
#[no_mangle]
pub static mut numspritelumps: c_int = 0;

/// Total number of wall textures loaded from TEXTURE1 and TEXTURE2.
///
/// Exported as `#[no_mangle]`.  Bounds all per-texture arrays and the hash
/// table.
#[no_mangle]
pub static mut numtextures: c_int = 0;

/// Array of pointers to runtime texture descriptors, length `numtextures`.
///
/// Allocated from the zone heap during `R_InitTextures`.  Private; accessed
/// only within this module.
static mut textures: *mut *mut texture_t = ptr::null_mut();

/// Hash table of texture pointers for O(1) name lookup, length `numtextures`.
///
/// Populated by `GenerateTextureHashTable`.  Each slot is the head of a
/// linked list chained through `texture_t::next`.  Private.
static mut textures_hashtable: *mut *mut texture_t = ptr::null_mut();

/// Per-texture column width mask (`width_rounded_up_to_power_of_two - 1`).
///
/// Used by `R_GetColumn` to wrap column indices: `col & texturewidthmask[tex]`.
/// Private; not exported to C.
static mut texturewidthmask: *mut c_int = ptr::null_mut();

/// Per-texture height in 16.16 fixed-point units, length `numtextures`.
///
/// Exported via `r_state.h` as `fixed_t *textureheight`.  Used by `r_segs.c`
/// and `p_floor.c` for texture-pegging calculations.
#[no_mangle]
pub static mut textureheight: *mut c_int = ptr::null_mut();

/// Per-texture total byte size of the composite column buffer, length `numtextures`.
///
/// A zero entry means the texture has no multi-patch columns and no composite
/// buffer is ever allocated.  Private.
static mut texturecompositesize: *mut c_int = ptr::null_mut();

/// Per-texture array of column-lump indices, each array has `texture.width` entries.
///
/// `texturecolumnlump[tex][col]` is the WAD lump number to use for column
/// `col` of texture `tex`, or `-1` if the column requires compositing.
/// Private.
static mut texturecolumnlump: *mut *mut c_short = ptr::null_mut();

/// Per-texture array of column byte offsets, each array has `texture.width` entries.
///
/// `texturecolumnofs[tex][col]` is the byte offset within the lump (or within
/// the composite buffer) for the start of column data.  Private.
static mut texturecolumnofs: *mut *mut c_ushort = ptr::null_mut();

/// Per-texture pointer to the composited column buffer, length `numtextures`.
///
/// Null until `R_GenerateComposite` is called for that texture.  Tagged
/// `PU_CACHE` so the zone allocator may evict it; `R_GetColumn` regenerates
/// on the next access.  Private.
static mut texturecomposite: *mut *mut u8 = ptr::null_mut();

/// Flat animation translation table, length `numflats + 1`.
///
/// Exported via `r_state.h` as `int *flattranslation`.  `p_spec.c` updates
/// entries during animated flat processing; `r_plane.c` uses
/// `flattranslation[pl->picnum]` when fetching the actual lump to draw.
#[no_mangle]
pub static mut flattranslation: *mut c_int = ptr::null_mut();

/// Texture animation translation table, length `numtextures + 1`.
///
/// Exported via `r_state.h` as `int *texturetranslation`.  `p_spec.c`
/// updates entries for animated textures; `r_segs.c` applies the translation
/// before calling `R_GetColumn`.
#[no_mangle]
pub static mut texturetranslation: *mut c_int = ptr::null_mut();

/// Per-sprite-lump width in 16.16 fixed-point units, length `numspritelumps`.
///
/// Exported via `r_state.h` as `fixed_t *spritewidth`.  Used by `r_things.c`
/// to compute screen-space sprite extents.
#[no_mangle]
pub static mut spritewidth: *mut c_int = ptr::null_mut();

/// Per-sprite-lump horizontal draw offset in 16.16 fixed-point units, length `numspritelumps`.
///
/// Exported via `r_state.h` as `fixed_t *spriteoffset`.  Used by `r_things.c`
/// to position sprites relative to the thing's world coordinates.
#[no_mangle]
pub static mut spriteoffset: *mut c_int = ptr::null_mut();

/// Per-sprite-lump vertical draw offset in 16.16 fixed-point units, length `numspritelumps`.
///
/// Exported via `r_state.h` as `fixed_t *spritetopoffset`.  Used by
/// `r_things.c` to compute the top screen row of each sprite.
#[no_mangle]
pub static mut spritetopoffset: *mut c_int = ptr::null_mut();

/// Pointer to the COLORMAP lump data: 34 colormaps of 256 bytes each.
///
/// Exported via `r_state.h` as `lighttable_t *colormaps`.  The renderer
/// indexes this as `colormaps + light_level * 256` to obtain a 256-entry
/// palette remapping table.  Used by `r_main.c`, `r_draw.c`, `r_plane.c`,
/// and `r_things.c`.
#[no_mangle]
pub static mut colormaps: *mut u8 = ptr::null_mut();

/// Total bytes of flat data touched during `R_PrecacheLevel`, for diagnostics.
///
/// Exported as `#[no_mangle]`.  Corresponds to `flatmemory` in `r_data.c`.
#[no_mangle]
pub static mut flatmemory: c_int = 0;

/// Total bytes of texture patch data touched during `R_PrecacheLevel`, for diagnostics.
///
/// Exported as `#[no_mangle]`.  Corresponds to `texturememory` in `r_data.c`.
#[no_mangle]
pub static mut texturememory: c_int = 0;

/// Total bytes of sprite lump data touched during `R_PrecacheLevel`, for diagnostics.
///
/// Exported as `#[no_mangle]`.  Corresponds to `spritememory` in `r_data.c`.
#[no_mangle]
pub static mut spritememory: c_int = 0;

// ---------------------------------------------------------------------------
// R_DrawColumnInCache
// ---------------------------------------------------------------------------

/// Copy a single patch column into a pre-allocated composite texture buffer.
///
/// Iterates over the post list in `patch` (terminated by `topdelta == 0xff`)
/// and `memcpy`s each run of pixels into `cache` at the correct vertical
/// offset.  Clips posts that extend above zero or below `cacheheight`.
///
/// # Parameters
///
/// - `patch` - pointer to the first `column_t` (post) of the column.
/// - `cache` - pointer to the start of the destination column inside the
///   composite buffer (i.e., `block + colofs[x]`).
/// - `originy` - vertical origin of the owning patch within the texture.
/// - `cacheheight` - height of the texture in pixels; used for clipping.
///
/// # Safety
///
/// - `patch` must point to a valid column terminated by a `0xff` topdelta.
/// - `cache` must have at least `cacheheight` bytes of writable storage.
/// - `originy + post.topdelta` must not underflow past `i32::MIN` (safe for
///   all legal WAD data).
unsafe fn R_DrawColumnInCache(
    patch: *mut column_t,
    cache: *mut u8,
    originy: c_int,
    cacheheight: c_int,
) {
    let mut patch = patch;
    while (*patch).topdelta != 0xff {
        let source = (patch as *mut u8).add(3);
        let mut count = (*patch).length as c_int;
        let mut position = originy + (*patch).topdelta as c_int;

        if position < 0 {
            count += position;
            position = 0;
        }
        if position + count > cacheheight {
            count = cacheheight - position;
        }
        if count > 0 {
            std::ptr::copy_nonoverlapping(source, cache.add(position as usize), count as usize);
        }
        patch = (patch as *mut u8).add((*patch).length as usize + 4) as *mut column_t;
    }
}

// ---------------------------------------------------------------------------
// R_GenerateComposite
// ---------------------------------------------------------------------------

/// Build the composite texture buffer for texture `texnum` from its patches.
///
/// Allocates a `PU_STATIC` zone block large enough for all composited columns
/// (size pre-computed by `R_GenerateLookup`), then calls
/// `R_DrawColumnInCache` for every column that requires compositing
/// (`texturecolumnlump[texnum][col] < 0`).  After compositing, the block is
/// downgraded to `PU_CACHE` so the zone allocator may evict it later.
///
/// Called lazily by `R_GetColumn` the first time a composited column of a
/// given texture is needed.
///
/// # Safety
///
/// - `texnum` must be in `0..numtextures`.
/// - `R_InitTextures` and `R_GenerateLookup` must have been called first.
/// - `texturecompositesize[texnum]` must be non-zero (ensured by
///   `R_GenerateLookup` for textures with overlapping patches).
#[no_mangle]
pub unsafe extern "C" fn R_GenerateComposite(texnum: c_int) {
    let texture = *textures.add(texnum as usize);

    let block = Z_Malloc(
        *texturecompositesize.add(texnum as usize),
        PU_STATIC,
        texturecomposite.add(texnum as usize) as *mut c_void,
    ) as *mut u8;

    let collump = *texturecolumnlump.add(texnum as usize);
    let colofs = *texturecolumnofs.add(texnum as usize);
    let patchcount = (*texture).patchcount as c_int;
    let patches_base = std::ptr::addr_of!((*texture).patches) as *mut texpatch_t;

    for i in 0..patchcount {
        let patch = patches_base.add(i as usize);
        let realpatch = W_CacheLumpNum((*patch).patch, PU_CACHE) as *mut patch_t;
        let x1 = (*patch).originx as c_int;
        let mut x2 = x1 + SHORT((*realpatch).width) as c_int;

        let mut x = if x1 < 0 { 0 } else { x1 };
        if x2 > (*texture).width as c_int {
            x2 = (*texture).width as c_int;
        }

        let columnofs = (realpatch as *mut u8).add(8) as *mut c_int;
        while x < x2 {
            if *collump.add(x as usize) >= 0 {
                x += 1;
                continue;
            }
            let patchcol = (realpatch as *mut u8)
                .add(LONG(*columnofs.add((x - x1) as usize)) as usize)
                as *mut column_t;
            R_DrawColumnInCache(
                patchcol,
                block.add(*colofs.add(x as usize) as usize),
                (*patch).originy as c_int,
                (*texture).height as c_int,
            );
            x += 1;
        }
    }

    Z_ChangeTag2(block as *mut c_void, PU_CACHE, ptr::null(), 0);
}

// ---------------------------------------------------------------------------
// R_GenerateLookup
// ---------------------------------------------------------------------------

/// Pre-compute per-column lump/offset lookup tables for texture `texnum`.
///
/// For each column of the texture:
/// - Counts how many patches cover it.
/// - If exactly one patch covers it, records the patch lump number and byte
///   offset so `R_GetColumn` can serve data directly from the WAD cache.
/// - If multiple patches overlap, sets `collump[col] = -1` and accumulates
///   `texturecompositesize` to reserve space for the later composite buffer.
///
/// Prints a warning (and returns early) if a column has no patch coverage.
/// Calls `I_Error` if the composite buffer would exceed 64 KiB.
///
/// Called once per texture by `R_InitTextures` during startup.
///
/// # Safety
///
/// - `texnum` must be in `0..numtextures`.
/// - `texturecolumnlump[texnum]` and `texturecolumnofs[texnum]` must already
///   be allocated (done by `R_InitTextures` before this call).
#[no_mangle]
pub unsafe extern "C" fn R_GenerateLookup(texnum: c_int) {
    let texture = *textures.add(texnum as usize);
    let width = (*texture).width as c_int;

    *texturecomposite.add(texnum as usize) = ptr::null_mut();
    *texturecompositesize.add(texnum as usize) = 0;
    let collump = *texturecolumnlump.add(texnum as usize);
    let colofs = *texturecolumnofs.add(texnum as usize);

    let mut patchcount_ptr: *mut u8 = ptr::null_mut();
    let _patchcount_arr = Z_Malloc(
        width,
        PU_STATIC,
        &mut patchcount_ptr as *mut *mut u8 as *mut c_void,
    ) as *mut u8;
    let patchcount = patchcount_ptr; // Z_Malloc wrote the allocated pointer here

    for x in 0..width {
        *patchcount.add(x as usize) = 0;
        *collump.add(x as usize) = 0;
    }

    let patches_base = std::ptr::addr_of!((*texture).patches) as *mut texpatch_t;
    for i in 0..(*texture).patchcount as c_int {
        let patch = patches_base.add(i as usize);
        let realpatch = W_CacheLumpNum((*patch).patch, PU_CACHE) as *mut patch_t;
        let x1 = (*patch).originx as c_int;
        let mut x2 = x1 + SHORT((*realpatch).width) as c_int;

        let mut x = if x1 < 0 { 0 } else { x1 };
        if x2 > width {
            x2 = width;
        }

        let columnofs = (realpatch as *mut u8).add(8) as *mut c_int;
        while x < x2 {
            *patchcount.add(x as usize) += 1;
            *collump.add(x as usize) = (*patch).patch as c_short;
            *colofs.add(x as usize) = (LONG(*columnofs.add((x - x1) as usize)) + 3) as c_ushort;
            x += 1;
        }
    }

    for x in 0..width {
        if *patchcount.add(x as usize) == 0 {
            libc::printf(
                c"R_GenerateLookup: column without a patch (%s)\n".as_ptr(),
                (*texture).name.as_ptr(),
            );
            Z_Free(patchcount as *mut c_void);
            return;
        }
        if *patchcount.add(x as usize) > 1 {
            *collump.add(x as usize) = -1;
            *colofs.add(x as usize) = *texturecompositesize.add(texnum as usize) as c_ushort;

            if *texturecompositesize.add(texnum as usize) > 0x10000 - (*texture).height as c_int {
                i_error!("R_GenerateLookup: texture {} is >64k", texnum);
            }
            *texturecompositesize.add(texnum as usize) += (*texture).height as c_int;
        }
    }

    Z_Free(patchcount as *mut c_void);
}

// ---------------------------------------------------------------------------
// R_GetColumn
// ---------------------------------------------------------------------------

/// Return a pointer to the pixel data for column `col` of texture `tex`.
///
/// Applies the width mask to wrap `col` into range, then looks up the result
/// in the pre-computed `texturecolumnlump` / `texturecolumnofs` tables:
/// - If `lump > 0`: data comes directly from the WAD cache (single-patch
///   column). Note: lump 0 is treated as composite even though it is a valid
///   WAD lump number — this matches the C original.
/// - Otherwise: calls `R_GenerateComposite` on the first access for that
///   texture, then returns into the composite buffer.
///
/// This is the hot-path column-fetch function; called from `r_segs.c`,
/// `r_plane.c`, and `r_things.c` on every rendered column.
///
/// # Safety
///
/// - `tex` must be in `0..numtextures`.
/// - `R_InitTextures` and `R_GenerateLookup` must have been called.
/// - The returned pointer is valid until the zone allocator evicts the
///   underlying lump or composite buffer.
#[no_mangle]
pub unsafe extern "C" fn R_GetColumn(tex: c_int, col: c_int) -> *mut u8 {
    let col = col & *texturewidthmask.add(tex as usize);
    let lump = *(*texturecolumnlump.add(tex as usize)).add(col as usize) as c_int;
    let ofs = *(*texturecolumnofs.add(tex as usize)).add(col as usize) as c_int;

    if lump > 0 {
        return (W_CacheLumpNum(lump, PU_CACHE) as *mut u8).add(ofs as usize);
    }

    if (*texturecomposite.add(tex as usize)).is_null() {
        R_GenerateComposite(tex);
    }

    (*texturecomposite.add(tex as usize)).add(ofs as usize)
}

// ---------------------------------------------------------------------------
// GenerateTextureHashTable
// ---------------------------------------------------------------------------

/// Build the hash table for O(1) texture name lookup.
///
/// Allocates `textures_hashtable` (length `numtextures`), then inserts every
/// texture into the table keyed by `W_LumpNameHash(name) % numtextures`.
/// Collisions are resolved by appending to the end of the bucket's linked
/// list (via `texture_t::next`), which preserves the vanilla Doom behaviour:
/// when two textures share a name, the one with the lower index wins because
/// it is at the head of the chain.
///
/// Called once at the end of `R_InitTextures`.
///
/// # Safety
///
/// - `textures` must be fully populated (`numtextures` valid pointers).
/// - Must not be called more than once per session (would leak the old table).
unsafe fn GenerateTextureHashTable() {
    textures_hashtable = Z_Malloc(
        ((std::mem::size_of::<*mut texture_t>() * numtextures as usize) as c_int) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut texture_t;

    for i in 0..numtextures as usize {
        *textures_hashtable.add(i) = ptr::null_mut();
    }

    for i in 0..numtextures as usize {
        let tex = *textures.add(i);
        (*tex).index = i as c_int;

        let key = (W_LumpNameHash((*tex).name.as_ptr()) % numtextures as c_uint) as usize;
        let mut rover = textures_hashtable.add(key);

        while !(*rover).is_null() {
            rover = std::ptr::addr_of_mut!((**rover).next);
        }

        (*tex).next = ptr::null_mut();
        *rover = tex;
    }
}

// ---------------------------------------------------------------------------
// R_InitTextures
// ---------------------------------------------------------------------------

/// Load and initialise all wall textures from the WAD.
///
/// Performs the following steps in order:
/// 1. Reads the PNAMES lump to build a patch-name-to-lump-number table.
/// 2. Reads TEXTURE1 (and TEXTURE2 if present) to obtain texture definitions.
/// 3. Allocates and populates the `textures`, `texturecolumnlump`,
///    `texturecolumnofs`, `texturecomposite`, `texturecompositesize`,
///    `texturewidthmask`, and `textureheight` arrays.
/// 4. Calls `R_GenerateLookup` for each texture to fill the column tables.
/// 5. Allocates `texturetranslation` (identity mapping, overridden later by
///    `p_spec.c` for animated textures).
/// 6. Calls `GenerateTextureHashTable` to enable O(1) name lookup.
///
/// Prints progress dots to stdout (if `I_ConsoleStdout` returns non-zero)
/// using the classic Doom "filling-the-box" animation.
///
/// Called by `R_InitData`, which is called by `r_main.c` (`R_Init`).
///
/// # Safety
///
/// - WAD must be fully loaded (`W_Init` must have been called).
/// - Must be called exactly once per process lifetime.
#[no_mangle]
pub unsafe extern "C" fn R_InitTextures() {
    let mut name: [c_char; 9] = [0; 9];

    let names = W_CacheLumpName(DEH_String(c"PNAMES".as_ptr()), PU_STATIC) as *mut c_int;
    let nummappatches = LONG(*names);
    let name_p = names.add(1) as *mut c_char;

    let patchlookup = Z_Malloc(
        (nummappatches as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;

    for i in 0..nummappatches as usize {
        M_StringCopy(name.as_mut_ptr(), name_p.add(i * 8), name.len());
        *patchlookup.add(i) = W_CheckNumForName(name.as_mut_ptr());
    }
    W_ReleaseLumpName(DEH_String(c"PNAMES".as_ptr()));

    let maptex1 = W_CacheLumpName(DEH_String(c"TEXTURE1".as_ptr()), PU_STATIC) as *mut c_int;
    let numtextures1 = LONG(*maptex1);
    let maxoff = W_LumpLength(W_GetNumForName(DEH_String(c"TEXTURE1".as_ptr())) as c_uint);
    let directory = maptex1.add(1);

    let mut maptex2: *mut c_int = ptr::null_mut();
    let mut numtextures2: c_int = 0;
    let mut maxoff2: c_int = 0;

    if W_CheckNumForName(DEH_String(c"TEXTURE2".as_ptr())) != -1 {
        maptex2 = W_CacheLumpName(DEH_String(c"TEXTURE2".as_ptr()), PU_STATIC) as *mut c_int;
        numtextures2 = LONG(*maptex2);
        maxoff2 = W_LumpLength(W_GetNumForName(DEH_String(c"TEXTURE2".as_ptr())) as c_uint);
    }

    numtextures = numtextures1 + numtextures2;

    textures = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut texture_t>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut texture_t;
    texturecolumnlump = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut c_short>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut c_short;
    texturecolumnofs = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut c_ushort>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut c_ushort;
    texturecomposite = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut u8>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut u8;
    texturecompositesize = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;
    texturewidthmask = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;
    textureheight = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;

    let temp1 = W_GetNumForName(DEH_String(c"S_START".as_ptr()));
    let temp2 = W_GetNumForName(DEH_String(c"S_END".as_ptr())) - 1;
    let temp3 = ((temp2 - temp1 + 63) / 64) + ((numtextures + 63) / 64);

    if I_ConsoleStdout() != 0 {
        libc::printf(c"[".as_ptr());
        for _ in 0..temp3 + 9 {
            libc::printf(c" ".as_ptr());
        }
        libc::printf(c"]".as_ptr());
        for _ in 0..temp3 + 10 {
            libc::printf(c"\x08".as_ptr());
        }
    }

    let mut maptex = maptex1;
    let mut maxoff = maxoff;
    let mut directory = directory;

    for i in 0..numtextures as usize {
        if (i & 63) == 0 {
            libc::printf(c".".as_ptr());
        }

        if i == numtextures1 as usize {
            maptex = maptex2;
            maxoff = maxoff2;
            directory = maptex2.add(1);
        }

        let offset = LONG(*directory) as isize;
        directory = directory.add(1);

        if offset > maxoff as isize {
            i_error!("R_InitTextures: bad texture directory");
        }

        let mtexture = (maptex as *mut u8).offset(offset) as *mut maptexture_t;
        let patchcount = SHORT((*mtexture).patchcount) as c_int;
        let texsize = std::mem::size_of::<texture_t>()
            + std::mem::size_of::<texpatch_t>() * (patchcount as usize - 1);
        let texture = Z_Malloc(texsize as c_int, PU_STATIC, ptr::null_mut()) as *mut texture_t;
        *textures.add(i) = texture;

        (*texture).width = SHORT((*mtexture).width);
        (*texture).height = SHORT((*mtexture).height);
        (*texture).patchcount = patchcount as i16;

        std::ptr::copy_nonoverlapping((*mtexture).name.as_ptr(), (*texture).name.as_mut_ptr(), 8);

        let mpatches_base = std::ptr::addr_of!((*mtexture).patches) as *mut mappatch_t;
        let tpatches_base = std::ptr::addr_of!((*texture).patches) as *mut texpatch_t;

        for j in 0..patchcount as usize {
            let mpatch = mpatches_base.add(j);
            let patch = tpatches_base.add(j);
            (*patch).originx = SHORT((*mpatch).originx);
            (*patch).originy = SHORT((*mpatch).originy);
            (*patch).patch = *patchlookup.add(SHORT((*mpatch).patch) as usize);
            if (*patch).patch == -1 {
                i_error!(
                    "R_InitTextures: Missing patch in texture {}",
                    CStr::from_ptr((*texture).name.as_ptr()).to_string_lossy()
                );
            }
        }

        let twidth = (*texture).width as c_int;
        *texturecolumnlump.add(i) = Z_Malloc(
            (twidth as usize * std::mem::size_of::<c_short>()) as c_int,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut c_short;
        *texturecolumnofs.add(i) = Z_Malloc(
            (twidth as usize * std::mem::size_of::<c_ushort>()) as c_int,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut c_ushort;

        let mut j = 1;
        while j * 2 <= twidth {
            j <<= 1;
        }
        *texturewidthmask.add(i) = j - 1;
        *textureheight.add(i) = ((*texture).height as c_int) << FRACBITS;
    }

    Z_Free(patchlookup as *mut c_void);
    W_ReleaseLumpName(DEH_String(c"TEXTURE1".as_ptr()));
    if !maptex2.is_null() {
        W_ReleaseLumpName(DEH_String(c"TEXTURE2".as_ptr()));
    }

    for i in 0..numtextures as usize {
        R_GenerateLookup(i as c_int);
    }

    texturetranslation = Z_Malloc(
        ((numtextures as usize + 1) * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;
    for i in 0..numtextures as usize {
        *texturetranslation.add(i) = i as c_int;
    }

    GenerateTextureHashTable();
}

// ---------------------------------------------------------------------------
// R_InitFlats
// ---------------------------------------------------------------------------

/// Locate the flat lump range in the WAD and initialise the animation translation table.
///
/// Sets `firstflat`, `lastflat`, and `numflats` from the `F_START` / `F_END`
/// marker lumps.  Allocates `flattranslation` as an identity mapping
/// (`flattranslation[i] = i`) that `p_spec.c` later updates for animated flats.
///
/// Called by `R_InitData`.
///
/// # Safety
///
/// - WAD must be fully loaded; `F_START` and `F_END` lumps must exist.
/// - Must be called exactly once per process lifetime.
#[no_mangle]
pub unsafe extern "C" fn R_InitFlats() {
    firstflat = W_GetNumForName(DEH_String(c"F_START".as_ptr())) + 1;
    lastflat = W_GetNumForName(DEH_String(c"F_END".as_ptr())) - 1;
    numflats = lastflat - firstflat + 1;

    flattranslation = Z_Malloc(
        ((numflats as usize + 1) * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;

    for i in 0..numflats as usize {
        *flattranslation.add(i) = i as c_int;
    }
}

// ---------------------------------------------------------------------------
// R_InitSpriteLumps
// ---------------------------------------------------------------------------

/// Load sprite lump header data and populate the sprite metric arrays.
///
/// Locates the sprite lump range (`S_START` / `S_END`), then reads the
/// `patch_t` header of each sprite lump to fill:
/// - `spritewidth[i]`     - patch width in 16.16 fixed-point.
/// - `spriteoffset[i]`    - left-offset in 16.16 fixed-point.
/// - `spritetopoffset[i]` - top-offset in 16.16 fixed-point.
///
/// Only the four-field patch header is read; the actual pixel columns are not
/// loaded at this stage (they are cached on demand during rendering).  Prints
/// a progress dot every 64 lumps.
///
/// Called by `R_InitData`.
///
/// # Safety
///
/// - WAD must be fully loaded; `S_START` and `S_END` lumps must exist.
/// - Must be called exactly once per process lifetime.
#[no_mangle]
pub unsafe extern "C" fn R_InitSpriteLumps() {
    firstspritelump = W_GetNumForName(DEH_String(c"S_START".as_ptr())) + 1;
    lastspritelump = W_GetNumForName(DEH_String(c"S_END".as_ptr())) - 1;
    numspritelumps = lastspritelump - firstspritelump + 1;

    spritewidth = Z_Malloc(
        (numspritelumps as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;
    spriteoffset = Z_Malloc(
        (numspritelumps as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;
    spritetopoffset = Z_Malloc(
        (numspritelumps as usize * std::mem::size_of::<c_int>()) as c_int,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut c_int;

    for i in 0..numspritelumps as usize {
        if (i & 63) == 0 {
            libc::printf(c".".as_ptr());
        }

        let patch = W_CacheLumpNum(firstspritelump + i as c_int, PU_CACHE) as *mut patch_t;
        *spritewidth.add(i) = (SHORT((*patch).width) as c_int) << FRACBITS;
        *spriteoffset.add(i) = (SHORT((*patch).leftoffset) as c_int) << FRACBITS;
        *spritetopoffset.add(i) = (SHORT((*patch).topoffset) as c_int) << FRACBITS;
    }
}

// ---------------------------------------------------------------------------
// R_InitColormaps
// ---------------------------------------------------------------------------

/// Load the COLORMAP lump and set the `colormaps` pointer.
///
/// The COLORMAP lump contains 34 light tables of 256 bytes each, stored as a
/// single contiguous block.  The renderer indexes it as
/// `colormaps + light_level * 256` to look up color remappings for a given
/// lighting level.  The lump is tagged `PU_STATIC` and is never freed.
///
/// Called by `R_InitData`.
///
/// # Safety
///
/// - WAD must be fully loaded; the `COLORMAP` lump must exist.
/// - Must be called exactly once per process lifetime.
#[no_mangle]
pub unsafe extern "C" fn R_InitColormaps() {
    let lump = W_GetNumForName(DEH_String(c"COLORMAP".as_ptr()));
    colormaps = W_CacheLumpNum(lump, PU_STATIC) as *mut u8;
}

// ---------------------------------------------------------------------------
// R_InitData
// ---------------------------------------------------------------------------

/// Locate and initialise all renderer data lumps from the WAD.
///
/// Calls, in order:
/// 1. `R_InitTextures` - wall textures.
/// 2. `R_InitFlats`    - floor/ceiling flats.
/// 3. `R_InitSpriteLumps` - sprite metrics.
/// 4. `R_InitColormaps` - lighting tables.
///
/// Prints a progress dot after each phase (in addition to the dots printed
/// by the individual init functions).
///
/// Called by `r_main.c` (`R_Init`) after the WAD has been loaded.
///
/// # Safety
///
/// - All of the above preconditions apply.
/// - Must be called exactly once per process lifetime.
#[no_mangle]
pub unsafe extern "C" fn R_InitData() {
    R_InitTextures();
    libc::printf(c".".as_ptr());
    R_InitFlats();
    libc::printf(c".".as_ptr());
    R_InitSpriteLumps();
    libc::printf(c".".as_ptr());
    R_InitColormaps();
}

// ---------------------------------------------------------------------------
// R_FlatNumForName
// ---------------------------------------------------------------------------

/// Look up a flat by name and return its flat index (lump number minus `firstflat`).
///
/// Calls `W_CheckNumForName`; if the lump does not exist, calls `I_Error`.
/// The returned value is suitable for use as an index into `flattranslation`.
///
/// Called by `g_game.c` to resolve `SKYFLATNAME` into `skyflatnum`, and by
/// various map-object and sector-setup code paths.
///
/// # Safety
///
/// - `name` must be a valid, null-terminated C string of at most 8 characters.
/// - `R_InitFlats` must have been called first.
#[no_mangle]
pub unsafe extern "C" fn R_FlatNumForName(name: *mut c_char) -> c_int {
    let i = W_CheckNumForName(name);
    if i == -1 {
        let mut namet: [c_char; 9] = [0; 9];
        std::ptr::copy_nonoverlapping(name, namet.as_mut_ptr(), 8);
        i_error!(
            "R_FlatNumForName: {} not found",
            CStr::from_ptr(namet.as_ptr()).to_string_lossy()
        );
    }
    i - firstflat
}

// ---------------------------------------------------------------------------
// R_CheckTextureNumForName
// ---------------------------------------------------------------------------

/// Look up a texture by name and return its index, or `-1` if not found.
///
/// A name starting with `'-'` is the "no texture" marker and returns `0`
/// immediately without a hash lookup.  Otherwise the hash table built by
/// `GenerateTextureHashTable` is used for O(1) average-case lookup.
///
/// Called by `p_setup.c` (and others) when loading map geometry; the return
/// value of `-1` signals that the texture slot is intentionally empty.
///
/// # Safety
///
/// - `name` must be a valid, null-terminated C string of at most 8 characters.
/// - `R_InitTextures` must have been called first.
#[no_mangle]
pub unsafe extern "C" fn R_CheckTextureNumForName(name: *mut c_char) -> c_int {
    if *name == b'-' as c_char {
        return 0;
    }

    let key = (W_LumpNameHash(name) % numtextures as c_uint) as usize;
    let mut texture = *textures_hashtable.add(key);

    while !texture.is_null() {
        if strncasecmp((*texture).name.as_ptr(), name, 8) == 0 {
            return (*texture).index;
        }
        texture = (*texture).next;
    }

    -1
}

// ---------------------------------------------------------------------------
// R_TextureNumForName
// ---------------------------------------------------------------------------

/// Look up a texture by name and return its index; calls `I_Error` if not found.
///
/// Wraps `R_CheckTextureNumForName` and aborts if the result is `-1`.
/// Used wherever a missing texture is a fatal error (e.g., side-def loading
/// in `p_setup.c`, switch definitions in `p_switch.c`).
///
/// # Safety
///
/// - `name` must be a valid, null-terminated C string of at most 8 characters.
/// - `R_InitTextures` must have been called first.
#[no_mangle]
pub unsafe extern "C" fn R_TextureNumForName(name: *mut c_char) -> c_int {
    let i = R_CheckTextureNumForName(name);
    if i == -1 {
        i_error!(
            "R_TextureNumForName: {} not found",
            CStr::from_ptr(name).to_string_lossy()
        );
    }
    i
}

// ---------------------------------------------------------------------------
// R_PrecacheLevel
// ---------------------------------------------------------------------------

/// Warm the WAD lump cache with all graphics used by the current level.
///
/// Skipped entirely during demo playback (`demoplayback != 0`).
///
/// Iterates over sectors, sidedef texture slots, and the thinker list to
/// build presence bitmaps for flats, textures, and sprites, then calls
/// `W_CacheLumpNum(PU_CACHE)` for each lump that is marked present.
/// Accumulates the total lump sizes into `flatmemory`, `texturememory`, and
/// `spritememory` for diagnostic purposes.
///
/// Note: the sky texture is always marked present regardless of which sectors
/// are in the level.
///
/// Called from `p_setup.c`'s `P_SetupLevel` after the level geometry is loaded.
///
/// # Safety
///
/// - All init functions (`R_InitData`, `p_setup` globals) must have run.
/// - Must be called from the game-loop thread; touches global mutable state.
#[no_mangle]
pub unsafe extern "C" fn R_PrecacheLevel() {
    if demoplayback != 0 {
        return;
    }

    // Precache flats
    let flatpresent = Z_Malloc(numflats, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numflats as usize {
        *flatpresent.add(i) = 0;
    }

    for i in 0..numsectors as usize {
        *flatpresent.add((*sectors.add(i)).floorpic as usize) = 1;
        *flatpresent.add((*sectors.add(i)).ceilingpic as usize) = 1;
    }

    flatmemory = 0;
    for i in 0..numflats as usize {
        if *flatpresent.add(i) != 0 {
            let lump = firstflat + i as c_int;
            flatmemory += (*lumpinfo.add(lump as usize)).size;
            W_CacheLumpNum(lump, PU_CACHE);
        }
    }
    Z_Free(flatpresent as *mut c_void);

    // Precache textures
    let texturepresent = Z_Malloc(numtextures, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numtextures as usize {
        *texturepresent.add(i) = 0;
    }

    for i in 0..numsides as usize {
        *texturepresent.add((*sides.add(i)).toptexture as usize) = 1;
        *texturepresent.add((*sides.add(i)).midtexture as usize) = 1;
        *texturepresent.add((*sides.add(i)).bottomtexture as usize) = 1;
    }
    *texturepresent.add(skytexture as usize) = 1;

    texturememory = 0;
    for i in 0..numtextures as usize {
        if *texturepresent.add(i) == 0 {
            continue;
        }
        let texture = *textures.add(i);
        for j in 0..(*texture).patchcount as usize {
            let lump = (*std::ptr::addr_of!((*texture).patches).add(j)).patch;
            texturememory += (*lumpinfo.add(lump as usize)).size;
            W_CacheLumpNum(lump, PU_CACHE);
        }
    }
    Z_Free(texturepresent as *mut c_void);

    // Precache sprites
    let spritepresent = Z_Malloc(numsprites, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numsprites as usize {
        *spritepresent.add(i) = 0;
    }

    let mut th = thinkercap.next;
    while !std::ptr::eq(th, std::ptr::addr_of!(thinkercap)) {
        if (*th).function.acp1.map(|f| f as usize) == Some(P_MobjThinker as *const () as usize) {
            let mobj = th as *mut mobj_t;
            *spritepresent.add((*mobj).sprite as usize) = 1;
        }
        th = (*th).next;
    }

    spritememory = 0;
    for i in 0..numsprites as usize {
        if *spritepresent.add(i) == 0 {
            continue;
        }
        let sprdef = (sprites as *mut spritedef_t).add(i);
        for j in 0..(*sprdef).numframes as usize {
            let sf = (*sprdef).spriteframes.add(j);
            for k in 0..8usize {
                let lump = firstspritelump + (*sf).lump[k] as c_int;
                spritememory += (*lumpinfo.add(lump as usize)).size;
                W_CacheLumpNum(lump, PU_CACHE);
            }
        }
    }
    Z_Free(spritepresent as *mut c_void);
}

// ---------------------------------------------------------------------------
// Anchor
// ---------------------------------------------------------------------------

/// Force the linker to retain all exported symbols in this module.
///
/// Rust's dead-code elimination would otherwise strip `#[no_mangle]` functions
/// that are called only from C.  This anchor function references every exported
/// function by address so the linker keeps them in the final binary.
///
/// # Safety
///
/// Must only be called from the C side during startup. Taking function
/// addresses is safe; no pointers are dereferenced.
#[no_mangle]
pub unsafe extern "C" fn R_Data_Link_Anchor() {
    let _ = R_GenerateComposite as *const () as usize;
    let _ = R_GenerateLookup as *const () as usize;
    let _ = R_GetColumn as *const () as usize;
    let _ = R_InitTextures as *const () as usize;
    let _ = R_InitFlats as *const () as usize;
    let _ = R_InitSpriteLumps as *const () as usize;
    let _ = R_InitColormaps as *const () as usize;
    let _ = R_InitData as *const () as usize;
    let _ = R_FlatNumForName as *const () as usize;
    let _ = R_CheckTextureNumForName as *const () as usize;
    let _ = R_TextureNumForName as *const () as usize;
    let _ = R_PrecacheLevel as *const () as usize;
}
