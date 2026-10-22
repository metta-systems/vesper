//! Rust port of vendor/doomgeneric/w_checksum.c.
//!
//! Computes a SHA-1 hash over the WAD directory: for every lump it folds
//! the name, owning-WAD index, file offset and size into a `SHA1Context`.
//! The resulting digest is used by chocolate-doom for netgame consistency
//! checks (so all peers verify they loaded the same lumps).
//!
//! The C version keeps `open_wadfiles` and `num_open_wadfiles` as a
//! `realloc`-grown global; the Rust port replaces those with a `Vec<*mut
//! c_void>` allocated locally inside [`W_Checksum`] for the duration of
//! the call. The numeric file-index assignment order is identical to the
//! C version, so the resulting digest is unchanged.

#![allow(non_upper_case_globals, non_snake_case)]

use std::ffi::c_void;

use crate::doom::{
    m_misc::M_StringCopy,
    sha1::{
        sha1_digest_t, SHA1Context, SHA1_Final, SHA1_Init, SHA1_UpdateInt32, SHA1_UpdateString,
    },
    w_wad::{lumpinfo, numlumps},
};

/// Mirror of the C `lumpinfo_t` layout used by [`W_Checksum`] to read
/// individual lump metadata out of the `lumpinfo` array.
///
/// Layout invariant: `#[repr(C)]` with the same field order, types and
/// padding as the canonical `lumpinfo_t` defined in `w_wad.h`. Only the
/// fields touched by checksumming are exposed; `cache` and `next` are
/// present to keep the struct size and field offsets matching the C
/// definition.
#[repr(C)]
pub struct LumpInfo {
    /// Eight-character lump name (not null-terminated in the WAD on-disk
    /// format; copied through a 9-byte buffer before hashing).
    pub name: [u8; 8],
    /// Pointer to the owning `wad_file_t`. Used as the key for assigning a
    /// stable numeric file index via `get_file_number`.
    pub wad_file: *mut c_void,
    /// Byte offset of the lump payload inside its WAD.
    pub position: i32,
    /// Lump size in bytes.
    pub size: i32,
    /// Cached zone-allocated copy of the lump, if any (unused by
    /// checksumming).
    pub cache: *mut c_void,
    /// Linked list pointer used by the WAD subsystem; unused here but
    /// retained for layout parity with the C struct.
    pub next: *mut LumpInfo,
}

/// Map a `wad_file` handle to a stable small integer for inclusion in the
/// checksum.
///
/// Looks up `handle` in `open_wadfiles`; if present, returns its index. If
/// absent, appends it and returns the new index. This makes the digest
/// depend only on the relative order in which distinct WAD files are first
/// encountered when iterating the lump directory, not on pointer values,
/// which keeps the hash stable across runs.
///
/// # Safety
///
/// `open_wadfiles` must be a valid mutable reference. `handle` is only
/// compared for equality and stored back into the vector, never
/// dereferenced.
unsafe fn get_file_number(handle: *mut c_void, open_wadfiles: &mut Vec<*mut c_void>) -> i32 {
    for (i, &wad) in open_wadfiles.iter().enumerate() {
        if wad == handle {
            return i as i32;
        }
    }

    let result = open_wadfiles.len() as i32;
    open_wadfiles.push(handle);
    result
}

/// Fold one lump's identifying fields into the running SHA-1 state.
///
/// Hashes (in order) the 9-byte null-padded name, the owning WAD's small
/// integer index (via [`get_file_number`]), the lump position and the
/// lump size. The exact ordering and width of each update must match the
/// C version byte-for-byte, otherwise the digest will diverge.
///
/// # Safety
///
/// `sha1_context` must point to an initialised [`SHA1Context`]. `lump`
/// must point to a valid [`LumpInfo`]. `open_wadfiles` must be a valid
/// mutable reference.
unsafe fn checksum_add_lump(
    sha1_context: *mut SHA1Context,
    lump: *mut LumpInfo,
    open_wadfiles: &mut Vec<*mut c_void>,
) {
    let lump = &*lump;

    let mut buf: [u8; 9] = [0; 9];
    M_StringCopy(buf.as_mut_ptr(), lump.name.as_ptr(), buf.len());

    SHA1_UpdateString(sha1_context, buf.as_mut_ptr());
    SHA1_UpdateInt32(
        sha1_context,
        get_file_number(lump.wad_file, open_wadfiles) as u32,
    );
    SHA1_UpdateInt32(sha1_context, lump.position as u32);
    SHA1_UpdateInt32(sha1_context, lump.size as u32);
}

/// Compute a SHA-1 digest over the entire WAD directory and write it to
/// `digest`.
///
/// Iterates `lumpinfo[0..numlumps]`, hashing each entry through
/// `checksum_add_lump`. The result is used by netgame code to detect
/// mismatched WAD loadouts between peers.
///
/// Differs from the C version in one detail: the `open_wadfiles` registry
/// is allocated as a local `Vec` rather than a process-wide
/// `realloc`-grown array. Behaviour is otherwise identical.
///
/// # Safety
///
/// `digest` must point to a valid `sha1_digest_t` array with room for
/// `SHA1_DIGEST_SIZE` bytes. The global `lumpinfo` and `numlumps` must be
/// initialised (i.e. `W_InitMultipleFiles` has run).
pub unsafe fn W_Checksum(digest: *mut sha1_digest_t) {
    let mut sha1_context = std::mem::MaybeUninit::<SHA1Context>::uninit();
    SHA1_Init(sha1_context.as_mut_ptr());

    let mut open_wadfiles: Vec<*mut c_void> = Vec::new();

    for i in 0..numlumps {
        let lump = lumpinfo.add(i as usize) as *mut LumpInfo;
        checksum_add_lump(sha1_context.as_mut_ptr(), lump, &mut open_wadfiles);
    }

    SHA1_Final((*digest).as_mut_ptr(), sha1_context.as_mut_ptr());
}
