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
//! # Shared state
//!
//! The C globals live in [`RenderDataState`], the renderer's `data`.  Most
//! correspond to `extern` declarations in `r_state.h` and are read by several
//! renderer and physics modules.  Exceptions: `lastflat` and `numflats` were
//! only declared locally in `p_spec.c`, not in `r_state.h`.  The tables are
//! zone allocations set up once by `R_InitData`; later writes (animated
//! texture translation, the composite cache) go through the table pointers,
//! so readers only need a shared borrow.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use {crate::i_error, std::ffi::c_void};

use {std::ptr, zerocopy::FromBytes};

use crate::engine::{
    math::fixed::{FRACBITS, fixed_t},
    zone::{PU_CACHE, PU_STATIC},
};

// ---------------------------------------------------------------------------
// Constants & helpers
// ---------------------------------------------------------------------------

/// Identity word-swap for little-endian targets (mirrors `i_swap.h` `LONG`).
///
/// Used for the raw `int` counts and offsets of the PNAMES and TEXTURE1/2
/// lumps; on all currently supported targets `i32` is already
/// little-endian, so this is a no-op.  The record fields of
/// `wad::format` carry their byte order themselves.
#[inline]
fn LONG(x: i32) -> i32 {
    x
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Runtime patch descriptor stored inside a `texture_t`.
///
/// Corresponds to `texpatch_t` in `r_data.c`.  `patch` is a WAD lump number
/// (resolved from the PNAMES index during `R_InitTextures`).
#[derive(Clone, Copy)]
struct texpatch_t {
    /// Horizontal origin of the patch within the texture, in pixels.
    originx: i16,
    /// Vertical origin of the patch within the texture, in pixels.
    originy: i16,
    /// WAD lump number of the patch graphic.
    patch: i32,
}

/// Runtime texture descriptor held in the `textures` pointer array (C layout).
///
/// Corresponds to `texture_t` in `r_data.c`.  The `patches` field is a C
/// flexible-array member: the struct is allocated with room for
/// `patchcount - 1` additional `texpatch_t` entries immediately after.
/// The `next` pointer links entries that hash to the same bucket in
/// `textures_hashtable`.
// repr(C) keeps `patches` the last field, so the extra entries allocated past
// the struct extend that flexible array instead of overlapping other fields.
#[repr(C)]
struct texture_t {
    /// Texture name, up to 8 ASCII characters, NUL-padded.
    name: [u8; 8],
    /// Texture width in pixels.
    width: i16,
    /// Texture height in pixels.
    height: i16,
    /// Index of this texture in the `textures` array; set by `GenerateTextureHashTable`.
    index: i32,
    /// Next entry in the hash-table chain for this bucket, or null.
    next: *mut texture_t,
    /// Number of `texpatch_t` entries that follow this struct in memory.
    patchcount: i16,
    /// First patch descriptor; additional patches follow contiguously.
    patches: texpatch_t,
}

// Nothing but tail padding follows `patches`.
const _: () = assert!(
    std::mem::size_of::<texture_t>() - std::mem::offset_of!(texture_t, patches)
        < std::mem::size_of::<texpatch_t>() + std::mem::align_of::<texture_t>()
);

// ---------------------------------------------------------------------------
// C library
// ---------------------------------------------------------------------------

use crate::engine::{
    dehacked::DEH_String,
    game::demoplayback,
    interface::system::I_ConsoleStdout,
    play::tick::{ThinkerId, ThinkerKind},
    render_3d::RENDERER,
    wad::{
        W_CacheLumpName, W_CacheLumpNum, W_CheckNumForName, W_GetNumForName, W_LumpLength,
        W_LumpNameHash, W_ReleaseLumpName, cache_lump_bytes, cache_patch_num,
        format::{Column, mappatch_t, maptexture_t},
        lump_name, lump_names_equal, lumpinfo,
    },
    world::World,
    zone::{Z_ChangeTag2, Z_Free, Z_Malloc},
};

// ---------------------------------------------------------------------------
// State defined by this module
// ---------------------------------------------------------------------------

/// Renderer data state (C `r_data.c` globals).
pub struct RenderDataState {
    /// WAD lump index of the first flat (the lump after `F_START`).
    ///
    /// Exported as `extern int firstflat` in `r_state.h`.  Used by `r_plane.c`
    /// to compute lump numbers from flat indices: `lump = firstflat + flatnum`.
    pub firstflat: i32,
    /// WAD lump index of the last flat (the lump before `F_END`).
    ///
    /// Not in `r_state.h`; declared locally in `p_spec.c`.  Together with
    /// `firstflat` it defines the flat lump range in the WAD.
    pub lastflat: i32,
    /// Total number of flat lumps (`lastflat - firstflat + 1`).
    ///
    /// Declared locally in `p_spec.c` (not in `r_state.h`) as `extern int numflats`
    /// for animation bounds checking.
    pub numflats: i32,
    /// WAD lump index of the first sprite lump (the lump after `S_START`).
    ///
    /// Exported via `r_state.h`.  Used by `r_things.c` and `f_finale.c` to
    /// convert sprite-relative lump indices to absolute WAD lump numbers:
    /// `lump = firstspritelump + relative`.
    pub firstspritelump: i32,
    /// WAD lump index of the last sprite lump (the lump before `S_END`).
    ///
    /// Exported via `r_state.h`.  Used by `r_things.c` when scanning the WAD for
    /// sprite frames.
    pub lastspritelump: i32,
    /// Total number of sprite lumps (`lastspritelump - firstspritelump + 1`).
    ///
    /// Exported via `r_state.h`.  Bounds the `spritewidth`, `spriteoffset`, and
    /// `spritetopoffset` arrays.
    pub numspritelumps: i32,
    /// Total number of wall textures loaded from TEXTURE1 and TEXTURE2.
    ///
    /// Bounds all per-texture arrays and the hash table.
    pub numtextures: i32,
    /// Array of pointers to runtime texture descriptors, length `numtextures`.
    ///
    /// Allocated from the zone heap during `R_InitTextures`.  Private; accessed
    /// only within this module.
    textures: *mut *mut texture_t,
    /// Hash table of texture pointers for O(1) name lookup, length `numtextures`.
    ///
    /// Populated by `GenerateTextureHashTable`.  Each slot is the head of a
    /// linked list chained through `texture_t::next`.  Private.
    textures_hashtable: *mut *mut texture_t,
    /// Per-texture column width mask (`width_rounded_up_to_power_of_two - 1`).
    ///
    /// Used by `R_GetColumn` to wrap column indices: `col & texturewidthmask[tex]`.
    /// Private; not exported to C.
    texturewidthmask: *mut i32,
    /// Per-texture height in 16.16 fixed-point units, length `numtextures`.
    ///
    /// Exported via `r_state.h` as `fixed_t *textureheight`.  Used by `r_segs.c`
    /// and `p_floor.c` for texture-pegging calculations.
    pub textureheight: *mut i32,
    /// Per-texture total byte size of the composite column buffer, length `numtextures`.
    ///
    /// A zero entry means the texture has no multi-patch columns and no composite
    /// buffer is ever allocated.  Private.
    texturecompositesize: *mut i32,
    /// Per-texture array of column-lump indices, each array has `texture.width` entries.
    ///
    /// `texturecolumnlump[tex][col]` is the WAD lump number to use for column
    /// `col` of texture `tex`, or `-1` if the column requires compositing.
    /// Private.
    texturecolumnlump: *mut *mut i16,
    /// Per-texture array of column byte offsets, each array has `texture.width` entries.
    ///
    /// `texturecolumnofs[tex][col]` is the byte offset within the lump (or within
    /// the composite buffer) for the start of column data.  Private.
    texturecolumnofs: *mut *mut u16,
    /// Per-texture pointer to the composited column buffer, length `numtextures`.
    ///
    /// Null until `R_GenerateComposite` is called for that texture.  Tagged
    /// `PU_CACHE` so the zone allocator may evict it (clearing the slot through
    /// its user pointer); `R_GetColumn` regenerates on the next access.
    /// Private.
    texturecomposite: *mut *mut u8,
    /// Flat animation translation table, length `numflats + 1`.
    ///
    /// Exported via `r_state.h` as `int *flattranslation`.  `p_spec.c` updates
    /// entries during animated flat processing; `r_plane.c` uses
    /// `flattranslation[pl->picnum]` when fetching the actual lump to draw.
    pub flattranslation: *mut i32,
    /// Texture animation translation table, length `numtextures + 1`.
    ///
    /// Exported via `r_state.h` as `int *texturetranslation`.  `p_spec.c`
    /// updates entries for animated textures; `r_segs.c` applies the translation
    /// before calling `R_GetColumn`.
    pub texturetranslation: *mut i32,
    /// Per-sprite-lump width in 16.16 fixed-point units, length `numspritelumps`.
    ///
    /// Exported via `r_state.h` as `fixed_t *spritewidth`.  Used by `r_things.c`
    /// to compute screen-space sprite extents.
    pub spritewidth: *mut i32,
    /// Per-sprite-lump horizontal draw offset in 16.16 fixed-point units, length `numspritelumps`.
    ///
    /// Exported via `r_state.h` as `fixed_t *spriteoffset`.  Used by `r_things.c`
    /// to position sprites relative to the thing's world coordinates.
    pub spriteoffset: *mut i32,
    /// Per-sprite-lump vertical draw offset in 16.16 fixed-point units, length `numspritelumps`.
    ///
    /// Exported via `r_state.h` as `fixed_t *spritetopoffset`.  Used by
    /// `r_things.c` to compute the top screen row of each sprite.
    pub spritetopoffset: *mut i32,
    /// Pointer to the COLORMAP lump data: 34 colormaps of 256 bytes each.
    ///
    /// Exported via `r_state.h` as `lighttable_t *colormaps`.  The renderer
    /// indexes this as `colormaps + light_level * 256` to obtain a 256-entry
    /// palette remapping table.  Used by `r_main.c`, `r_draw.c`, `r_plane.c`,
    /// and `r_things.c`.
    pub colormaps: *mut u8,
    /// Total bytes of flat data touched during `R_PrecacheLevel`, for diagnostics.
    ///
    /// Corresponds to `flatmemory` in `r_data.c`.
    pub flatmemory: i32,
    /// Total bytes of texture patch data touched during `R_PrecacheLevel`, for diagnostics.
    ///
    /// Corresponds to `texturememory` in `r_data.c`.
    pub texturememory: i32,
    /// Total bytes of sprite lump data touched during `R_PrecacheLevel`, for diagnostics.
    ///
    /// Corresponds to `spritememory` in `r_data.c`.
    pub spritememory: i32,
}

impl RenderDataState {
    /// The state before `R_InitData`: no tables, all counts zero.
    pub const fn new() -> Self {
        Self {
            firstflat: 0,
            lastflat: 0,
            numflats: 0,
            firstspritelump: 0,
            lastspritelump: 0,
            numspritelumps: 0,
            numtextures: 0,
            textures: ptr::null_mut(),
            textures_hashtable: ptr::null_mut(),
            texturewidthmask: ptr::null_mut(),
            textureheight: ptr::null_mut(),
            texturecompositesize: ptr::null_mut(),
            texturecolumnlump: ptr::null_mut(),
            texturecolumnofs: ptr::null_mut(),
            texturecomposite: ptr::null_mut(),
            flattranslation: ptr::null_mut(),
            texturetranslation: ptr::null_mut(),
            spritewidth: ptr::null_mut(),
            spriteoffset: ptr::null_mut(),
            spritetopoffset: ptr::null_mut(),
            colormaps: ptr::null_mut(),
            flatmemory: 0,
            texturememory: 0,
            spritememory: 0,
        }
    }
}

impl Default for RenderDataState {
    fn default() -> Self {
        Self::new()
    }
}

/// Height of wall texture `texture` in 16.16 fixed point (C
/// `textureheight[texture]`).
///
/// # Panics
///
/// If `texture` is not in `0..numtextures` (always so before
/// `R_InitTextures` has run).
pub fn texture_height(texture: usize) -> fixed_t {
    let renderer = RENDERER.borrow();
    let data = &renderer.data;
    assert!(
        texture < data.numtextures as usize,
        "texture {texture} out of range (numtextures {})",
        data.numtextures
    );
    // SAFETY: `R_InitTextures` set `numtextures` and allocated and filled
    // that many heights in a static zone block, which is never freed.
    unsafe { *data.textureheight.add(texture) }
}

// ---------------------------------------------------------------------------
// R_DrawColumnInCache
// ---------------------------------------------------------------------------

/// Copy a single patch column into a pre-allocated composite texture buffer.
///
/// Copies each post of `column` into `cache` at the correct vertical
/// offset, clipping posts that extend above zero or below the end of
/// `cache`.
///
/// # Parameters
///
/// - `column` - the posts of the patch column.
/// - `cache` - the destination column inside the composite buffer (i.e.,
///   `block[colofs[x]..]`), one byte per texture row.
/// - `originy` - vertical origin of the owning patch within the texture.
fn R_DrawColumnInCache(column: Column<'_>, cache: &mut [u8], originy: i32) {
    let cacheheight = cache.len() as i32;
    for post in column {
        let mut count = post.pixels.len() as i32;
        let mut position = originy + post.topdelta as i32;

        // As in C, a post clipped at the top still copies from its first
        // pixel rather than skipping the rows above the texture.
        if position < 0 {
            count += position;
            position = 0;
        }
        if position + count > cacheheight {
            count = cacheheight - position;
        }
        if count > 0 {
            let position = position as usize;
            let count = count as usize;
            cache[position..position + count].copy_from_slice(&post.pixels[..count]);
        }
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
pub unsafe fn R_GenerateComposite(data: &RenderDataState, texnum: i32) {
    let texture = *data.textures.add(texnum as usize);

    let compositesize = *data.texturecompositesize.add(texnum as usize);
    let block = Z_Malloc(
        compositesize,
        PU_STATIC,
        data.texturecomposite.add(texnum as usize) as *mut c_void,
    ) as *mut u8;
    // The block is PU_STATIC until the end, so later allocations (the patch
    // cache misses below) cannot purge it.
    let composite = std::slice::from_raw_parts_mut(block, compositesize as usize);

    let collump = *data.texturecolumnlump.add(texnum as usize);
    let colofs = *data.texturecolumnofs.add(texnum as usize);
    let patchcount = (*texture).patchcount as i32;
    let patches_base = std::ptr::addr_of!((*texture).patches) as *mut texpatch_t;
    let texture_height = (*texture).height as usize;

    for i in 0..patchcount {
        let patch = patches_base.add(i as usize);
        // Used before the next patch's cache miss can purge it.
        let realpatch = cache_patch_num((*patch).patch, PU_CACHE);
        let x1 = (*patch).originx as i32;
        let mut x2 = x1 + realpatch.width();

        let mut x = if x1 < 0 { 0 } else { x1 };
        if x2 > (*texture).width as i32 {
            x2 = (*texture).width as i32;
        }

        while x < x2 {
            if *collump.add(x as usize) >= 0 {
                x += 1;
                continue;
            }
            let column_start = *colofs.add(x as usize) as usize;
            R_DrawColumnInCache(
                realpatch.column((x - x1) as usize),
                &mut composite[column_start..column_start + texture_height],
                (*patch).originy as i32,
            );
            x += 1;
        }
    }

    Z_ChangeTag2(block as *mut c_void, PU_CACHE);
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
pub unsafe fn R_GenerateLookup(data: &RenderDataState, texnum: i32) {
    let texture = *data.textures.add(texnum as usize);
    let width = (*texture).width as i32;

    *data.texturecomposite.add(texnum as usize) = ptr::null_mut();
    *data.texturecompositesize.add(texnum as usize) = 0;
    let collump = *data.texturecolumnlump.add(texnum as usize);
    let colofs = *data.texturecolumnofs.add(texnum as usize);

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
    for i in 0..(*texture).patchcount as i32 {
        let patch = patches_base.add(i as usize);
        // Used before the next patch's cache miss can purge it.
        let realpatch = cache_patch_num((*patch).patch, PU_CACHE);
        let x1 = (*patch).originx as i32;
        let mut x2 = x1 + realpatch.width();

        let mut x = if x1 < 0 { 0 } else { x1 };
        if x2 > width {
            x2 = width;
        }

        while x < x2 {
            *patchcount.add(x as usize) += 1;
            *collump.add(x as usize) = (*patch).patch as i16;
            *colofs.add(x as usize) = (realpatch.column_offset((x - x1) as usize) + 3) as u16;
            x += 1;
        }
    }

    let compositesize = data.texturecompositesize.add(texnum as usize);
    for x in 0..width {
        if *patchcount.add(x as usize) == 0 {
            println!(
                "R_GenerateLookup: column without a patch ({})",
                String::from_utf8_lossy(lump_name(&(*texture).name))
            );
            Z_Free(patchcount as *mut c_void);
            return;
        }
        if *patchcount.add(x as usize) > 1 {
            *collump.add(x as usize) = -1;
            *colofs.add(x as usize) = *compositesize as u16;

            if *compositesize > 0x10000 - (*texture).height as i32 {
                i_error!("R_GenerateLookup: texture {} is >64k", texnum);
            }
            *compositesize += (*texture).height as i32;
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
/// This is the hot-path column-fetch function; called from `r_segs.c` and
/// `r_plane.c` on every rendered column, so the caller borrows `data` once
/// for the whole wall or plane.
///
/// # Safety
///
/// - `tex` must be in `0..numtextures`.
/// - `R_InitTextures` and `R_GenerateLookup` must have been called.
/// - The returned pointer is valid until the zone allocator evicts the
///   underlying lump or composite buffer.
pub unsafe fn R_GetColumn(data: &RenderDataState, tex: i32, col: i32) -> *mut u8 {
    let col = col & *data.texturewidthmask.add(tex as usize);
    let lump = *(*data.texturecolumnlump.add(tex as usize)).add(col as usize) as i32;
    let ofs = *(*data.texturecolumnofs.add(tex as usize)).add(col as usize) as i32;

    if lump > 0 {
        return (W_CacheLumpNum(lump, PU_CACHE) as *mut u8).add(ofs as usize);
    }

    if (*data.texturecomposite.add(tex as usize)).is_null() {
        R_GenerateComposite(data, tex);
    }

    (*data.texturecomposite.add(tex as usize)).add(ofs as usize)
}

/// Return the posts of column `col` of masked texture `tex`, which C read
/// as `R_GetColumn(tex, col) - 3`, a `column_t` post list.
///
/// A single-patch column is the post list of the patch column in the WAD
/// cache. A composited column holds bare pixels without post headers, and
/// C read those pixels as posts (the "Medusa" glitch of multi-patch masked
/// textures); this reads them the same way, bounded by the composite
/// buffer. Only the first composited column differs: C started 3 bytes
/// before the buffer, inside the zone block header, and this starts at the
/// buffer's first byte.
///
/// # Safety
///
/// As for [`R_GetColumn`]: the column is valid only until the zone
/// allocator evicts the underlying lump or composite buffer, that is until
/// the next zone allocation.
pub unsafe fn R_GetMaskedColumn<'a>(data: &RenderDataState, tex: i32, col: i32) -> Column<'a> {
    let col = col & *data.texturewidthmask.add(tex as usize);
    let lump = *(*data.texturecolumnlump.add(tex as usize)).add(col as usize) as i32;
    let ofs = *(*data.texturecolumnofs.add(tex as usize)).add(col as usize) as usize;

    if lump > 0 {
        return Column::at_offset(cache_lump_bytes(lump, PU_CACHE), ofs.saturating_sub(3));
    }

    if (*data.texturecomposite.add(tex as usize)).is_null() {
        R_GenerateComposite(data, tex);
    }

    let composite = std::slice::from_raw_parts(
        *data.texturecomposite.add(tex as usize),
        *data.texturecompositesize.add(tex as usize) as usize,
    );
    Column::at_offset(composite, ofs.saturating_sub(3))
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
unsafe fn GenerateTextureHashTable(data: &mut RenderDataState) {
    let numtextures = data.numtextures;
    data.textures_hashtable = Z_Malloc(
        ((std::mem::size_of::<*mut texture_t>() * numtextures as usize) as i32) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut texture_t;

    for i in 0..numtextures as usize {
        *data.textures_hashtable.add(i) = ptr::null_mut();
    }

    for i in 0..numtextures as usize {
        let tex = *data.textures.add(i);
        (*tex).index = i as i32;

        let key = (W_LumpNameHash(&(*tex).name) % numtextures as u32) as usize;
        let mut rover = data.textures_hashtable.add(key);

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
pub unsafe fn R_InitTextures(data: &mut RenderDataState) {
    let names = W_CacheLumpName(DEH_String(b"PNAMES"), PU_STATIC) as *mut i32;
    let nummappatches = LONG(*names);
    let name_p = names.add(1) as *mut u8;

    let patchlookup = Z_Malloc(
        (nummappatches as usize * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;

    for i in 0..nummappatches as usize {
        // PNAMES entries are 8-byte names, not NUL-terminated when full.
        let name = std::slice::from_raw_parts(name_p.add(i * 8), 8);
        *patchlookup.add(i) = W_CheckNumForName(name);
    }
    W_ReleaseLumpName(DEH_String(b"PNAMES"));

    let maptex1 = W_CacheLumpName(DEH_String(b"TEXTURE1"), PU_STATIC) as *mut i32;
    let numtextures1 = LONG(*maptex1);
    let maxoff = W_LumpLength(W_GetNumForName(DEH_String(b"TEXTURE1")) as u32);
    let directory = maptex1.add(1);

    let mut maptex2: *mut i32 = ptr::null_mut();
    let mut numtextures2: i32 = 0;
    let mut maxoff2: i32 = 0;

    if W_CheckNumForName(DEH_String(b"TEXTURE2")) != -1 {
        maptex2 = W_CacheLumpName(DEH_String(b"TEXTURE2"), PU_STATIC) as *mut i32;
        numtextures2 = LONG(*maptex2);
        maxoff2 = W_LumpLength(W_GetNumForName(DEH_String(b"TEXTURE2")) as u32);
    }

    let numtextures = numtextures1 + numtextures2;
    data.numtextures = numtextures;

    data.textures = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut texture_t>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut texture_t;
    data.texturecolumnlump = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut i16>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut i16;
    data.texturecolumnofs = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut u16>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut u16;
    data.texturecomposite = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<*mut u8>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut *mut u8;
    data.texturecompositesize = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;
    data.texturewidthmask = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;
    data.textureheight = Z_Malloc(
        (numtextures as usize * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;

    let temp1 = W_GetNumForName(DEH_String(b"S_START"));
    let temp2 = W_GetNumForName(DEH_String(b"S_END")) - 1;
    let temp3 = ((temp2 - temp1 + 63) / 64) + ((numtextures + 63) / 64);

    if I_ConsoleStdout() != 0 {
        print!("[");
        for _ in 0..temp3 + 9 {
            print!(" ");
        }
        print!("]");
        for _ in 0..temp3 + 10 {
            print!("\x08");
        }
    }

    let mut maptex = maptex1;
    let mut maxoff = maxoff;
    let mut directory = directory;

    for i in 0..numtextures as usize {
        if (i & 63) == 0 {
            print!(".");
        }

        if i == numtextures1 as usize {
            maptex = maptex2;
            maxoff = maxoff2;
            directory = maptex2.add(1);
        }

        // A negative offset wraps to a huge one and fails the bound check.
        let offset = LONG(*directory) as usize;
        directory = directory.add(1);

        if offset > maxoff as usize {
            i_error!("R_InitTextures: bad texture directory");
        }

        let texture_lump = std::slice::from_raw_parts(maptex as *const u8, maxoff as usize);
        let Ok((mtexture, after_header)) = maptexture_t::ref_from_prefix(&texture_lump[offset..])
        else {
            i_error!(
                "R_InitTextures: truncated texture definition at offset {}",
                offset
            );
        };
        let patchcount = mtexture.patchcount.get() as i32;
        let Ok((mpatches, _)) =
            <[mappatch_t]>::ref_from_prefix_with_elems(after_header, patchcount as usize)
        else {
            i_error!(
                "R_InitTextures: truncated patch list in texture {}",
                String::from_utf8_lossy(lump_name(&mtexture.name))
            );
        };
        let texsize = std::mem::size_of::<texture_t>()
            + std::mem::size_of::<texpatch_t>() * (patchcount as usize - 1);
        let texture = Z_Malloc(texsize as i32, PU_STATIC, ptr::null_mut()) as *mut texture_t;
        *data.textures.add(i) = texture;

        (*texture).width = mtexture.width.get();
        (*texture).height = mtexture.height.get();
        (*texture).patchcount = patchcount as i16;
        (*texture).name = mtexture.name;

        let tpatches_base = std::ptr::addr_of!((*texture).patches) as *mut texpatch_t;

        for (j, mpatch) in mpatches.iter().enumerate() {
            let patch = tpatches_base.add(j);
            (*patch).originx = mpatch.originx.get();
            (*patch).originy = mpatch.originy.get();
            (*patch).patch = *patchlookup.add(mpatch.patch.get() as usize);
            if (*patch).patch == -1 {
                i_error!(
                    "R_InitTextures: Missing patch in texture {}",
                    String::from_utf8_lossy(lump_name(&(*texture).name))
                );
            }
        }

        let twidth = (*texture).width as i32;
        *data.texturecolumnlump.add(i) = Z_Malloc(
            (twidth as usize * std::mem::size_of::<i16>()) as i32,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut i16;
        *data.texturecolumnofs.add(i) = Z_Malloc(
            (twidth as usize * std::mem::size_of::<u16>()) as i32,
            PU_STATIC,
            ptr::null_mut(),
        ) as *mut u16;

        let mut j = 1;
        while j * 2 <= twidth {
            j <<= 1;
        }
        *data.texturewidthmask.add(i) = j - 1;
        *data.textureheight.add(i) = ((*texture).height as i32) << FRACBITS;
    }

    Z_Free(patchlookup as *mut c_void);
    W_ReleaseLumpName(DEH_String(b"TEXTURE1"));
    if !maptex2.is_null() {
        W_ReleaseLumpName(DEH_String(b"TEXTURE2"));
    }

    for i in 0..numtextures as usize {
        R_GenerateLookup(data, i as i32);
    }

    data.texturetranslation = Z_Malloc(
        ((numtextures as usize + 1) * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;
    for i in 0..numtextures as usize {
        *data.texturetranslation.add(i) = i as i32;
    }

    GenerateTextureHashTable(data);
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
pub unsafe fn R_InitFlats(data: &mut RenderDataState) {
    data.firstflat = W_GetNumForName(DEH_String(b"F_START")) + 1;
    data.lastflat = W_GetNumForName(DEH_String(b"F_END")) - 1;
    data.numflats = data.lastflat - data.firstflat + 1;

    data.flattranslation = Z_Malloc(
        ((data.numflats as usize + 1) * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;

    for i in 0..data.numflats as usize {
        *data.flattranslation.add(i) = i as i32;
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
/// Only the patch header is used (the [`Patch`](crate::engine::wad::format::Patch)
/// view checks that the column table fits the lump); the pixel columns are
/// read on demand during rendering.  Prints a progress dot every 64 lumps.
///
/// Called by `R_InitData`.
///
/// # Safety
///
/// - WAD must be fully loaded; `S_START` and `S_END` lumps must exist.
/// - Must be called exactly once per process lifetime.
pub unsafe fn R_InitSpriteLumps(data: &mut RenderDataState) {
    data.firstspritelump = W_GetNumForName(DEH_String(b"S_START")) + 1;
    data.lastspritelump = W_GetNumForName(DEH_String(b"S_END")) - 1;
    data.numspritelumps = data.lastspritelump - data.firstspritelump + 1;

    let numspritelumps = data.numspritelumps as usize;
    data.spritewidth = Z_Malloc(
        (numspritelumps * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;
    data.spriteoffset = Z_Malloc(
        (numspritelumps * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;
    data.spritetopoffset = Z_Malloc(
        (numspritelumps * std::mem::size_of::<i32>()) as i32,
        PU_STATIC,
        ptr::null_mut(),
    ) as *mut i32;

    for i in 0..numspritelumps {
        if (i & 63) == 0 {
            print!(".");
        }

        // Read at once, before the next sprite's cache miss can purge it.
        let patch = cache_patch_num(data.firstspritelump + i as i32, PU_CACHE);
        *data.spritewidth.add(i) = patch.width() << FRACBITS;
        *data.spriteoffset.add(i) = patch.leftoffset() << FRACBITS;
        *data.spritetopoffset.add(i) = patch.topoffset() << FRACBITS;
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
pub unsafe fn R_InitColormaps(data: &mut RenderDataState) {
    let lump = W_GetNumForName(DEH_String(b"COLORMAP"));
    data.colormaps = W_CacheLumpNum(lump, PU_STATIC) as *mut u8;
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
pub unsafe fn R_InitData(data: &mut RenderDataState) {
    R_InitTextures(data);
    print!(".");
    R_InitFlats(data);
    print!(".");
    R_InitSpriteLumps(data);
    print!(".");
    R_InitColormaps(data);
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
/// `R_InitFlats` must have been called first.
pub unsafe fn R_FlatNumForName(name: &[u8]) -> i32 {
    let i = W_CheckNumForName(name);
    if i == -1 {
        i_error!(
            "R_FlatNumForName: {} not found",
            String::from_utf8_lossy(lump_name(name))
        );
    }
    i - RENDERER.borrow().data.firstflat
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
/// `R_InitTextures` must have been called first.
pub unsafe fn R_CheckTextureNumForName(name: &[u8]) -> i32 {
    if name.first() == Some(&b'-') {
        return 0;
    }

    let renderer = RENDERER.borrow();
    let data = &renderer.data;
    let key = (W_LumpNameHash(name) % data.numtextures as u32) as usize;
    let mut texture = *data.textures_hashtable.add(key);

    while !texture.is_null() {
        if lump_names_equal(&(*texture).name, name) {
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
/// `R_InitTextures` must have been called first.
pub unsafe fn R_TextureNumForName(name: &[u8]) -> i32 {
    let i = R_CheckTextureNumForName(name);
    if i == -1 {
        i_error!(
            "R_TextureNumForName: {} not found",
            String::from_utf8_lossy(lump_name(name))
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
/// - `R_InitData` must have run and `world`'s level must be loaded.
/// - Must be called from the game-loop thread; touches global mutable state.
pub unsafe fn R_PrecacheLevel(world: &World) {
    if demoplayback() != 0 {
        return;
    }

    let level = &world.level;
    let sectors = &level.sectors[..level.numsectors as usize];
    let sides = &level.sides[..];
    let mut renderer = RENDERER.borrow_mut();
    let renderer = &mut *renderer;
    let data = &mut renderer.data;
    let numflats = data.numflats;
    let numtextures = data.numtextures;

    // Precache flats
    let flatpresent = Z_Malloc(numflats, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numflats as usize {
        *flatpresent.add(i) = 0;
    }

    for sector in sectors {
        *flatpresent.add(sector.floorpic as usize) = 1;
        *flatpresent.add(sector.ceilingpic as usize) = 1;
    }

    data.flatmemory = 0;
    for i in 0..numflats as usize {
        if *flatpresent.add(i) != 0 {
            let lump = data.firstflat + i as i32;
            data.flatmemory += (*lumpinfo().add(lump as usize)).size;
            W_CacheLumpNum(lump, PU_CACHE);
        }
    }
    Z_Free(flatpresent as *mut c_void);

    // Precache textures
    let texturepresent = Z_Malloc(numtextures, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numtextures as usize {
        *texturepresent.add(i) = 0;
    }

    for side in sides {
        *texturepresent.add(side.toptexture as usize) = 1;
        *texturepresent.add(side.midtexture as usize) = 1;
        *texturepresent.add(side.bottomtexture as usize) = 1;
    }
    *texturepresent.add(renderer.sky.skytexture as usize) = 1;

    data.texturememory = 0;
    for i in 0..numtextures as usize {
        if *texturepresent.add(i) == 0 {
            continue;
        }
        let texture = *data.textures.add(i);
        for j in 0..(*texture).patchcount as usize {
            let lump = (*std::ptr::addr_of!((*texture).patches).add(j)).patch;
            data.texturememory += (*lumpinfo().add(lump as usize)).size;
            W_CacheLumpNum(lump, PU_CACHE);
        }
    }
    Z_Free(texturepresent as *mut c_void);

    // Precache sprites
    let (numsprites, sprites) = (renderer.sprites.numsprites, renderer.sprites.sprites);
    let spritepresent = Z_Malloc(numsprites, PU_STATIC, ptr::null_mut()) as *mut u8;
    for i in 0..numsprites as usize {
        *spritepresent.add(i) = 0;
    }

    for &thinker in &world.thinkers.list {
        if let ThinkerId::Mobj(mobj) = thinker
            && thinker.kind(world) == ThinkerKind::Mobj
        {
            *spritepresent.add(world.mobjs[mobj].sprite as usize) = 1;
        }
    }

    data.spritememory = 0;
    for i in 0..numsprites as usize {
        if *spritepresent.add(i) == 0 {
            continue;
        }
        let sprdef = sprites.add(i);
        for j in 0..(*sprdef).numframes as usize {
            let sf = (*sprdef).spriteframes.add(j);
            for k in 0..8usize {
                let lump = data.firstspritelump + (*sf).lump[k] as i32;
                data.spritememory += (*lumpinfo().add(lump as usize)).size;
                W_CacheLumpNum(lump, PU_CACHE);
            }
        }
    }
    Z_Free(spritepresent as *mut c_void);
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::engine::wad::format::{Patch, tests::build_patch_lump},
    };

    /// Posts land at `originy + topdelta` in the composite column and are
    /// cut at its bottom; a post cut at the top still copies from its first
    /// pixel, as in C.
    #[test]
    fn column_in_cache_clips_like_c() {
        let lump = build_patch_lump(8, 0, 0, &[&[(0, &[1, 2, 3]), (4, &[4, 5, 6])]]);
        let patch = Patch::from_bytes(&lump).unwrap();

        let mut cache = [0u8; 6];
        R_DrawColumnInCache(patch.column(0), &mut cache, 1);
        assert_eq!(cache, [0, 1, 2, 3, 0, 4]);

        let mut cache = [0u8; 6];
        R_DrawColumnInCache(patch.column(0), &mut cache, -2);
        assert_eq!(cache, [1, 0, 4, 5, 6, 0]);
    }
}
