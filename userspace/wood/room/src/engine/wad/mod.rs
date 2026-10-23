//! Rust port of vendor/doomgeneric/w_wad.c.
//!
//! WAD file header / directory parsing, lump lookup, and caching.
//!
//! The global `lumpinfo` array and its companion `numlumps` /
//! `lumphash` form the in-memory directory of every lump loaded from
//! all WADs. `W_AddFile` is the entry point that appends a WAD or
//! single-lump file; `W_CheckNumForName` and `W_GetNumForName`
//! resolve lump names to indices; `W_CacheLumpNum` returns a
//! reusable pointer to the lump's bytes, populating the zone-managed
//! cache on first miss. The hash table built by `W_GenerateHashTable`
//! is consulted in preference to the linear scan once present.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

pub mod checksum;
pub mod file;
pub mod format;
pub mod main;

use {
    crate::{
        engine::{
            doom::{iwad::D_SuggestGameName, mode::D_GameMissionString},
            global::Global,
            interface::video::{I_BeginRead, I_EndRead},
            misc::{M_ExtractFileBase, until_nul},
            wad::{
                file::{W_OpenFile, W_Read, wad_file_t},
                format::{Patch, filelump_t, wadinfo_t},
            },
            zone::{PU_CACHE, PU_STATIC, Z_ChangeTag2, Z_ChangeUser, Z_Free, Z_Malloc},
        },
        i_error,
    },
    std::{ffi::c_void, ptr},
    zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, little_endian::I32},
};

/// One entry in the global lump directory. Mirrors `lumpinfo_t` in
/// `w_wad.h`.
pub struct lumpinfo_t {
    /// 8-char ASCII lump name, **not** NUL-terminated when full.
    pub name: [u8; 8],
    /// File the lump lives in.
    pub wad_file: *mut wad_file_t,
    /// Offset of the lump payload inside the file, in bytes.
    pub position: i32,
    /// Payload size in bytes.
    pub size: i32,
    /// Zone-allocated cache pointer, or null if not yet loaded.
    /// Memory-mapped files leave this null; `W_CacheLumpNum`
    /// returns a pointer into the mapping directly.
    pub cache: *mut c_void,
    /// Next entry in the per-hash-bucket chain when `lumphash` is
    /// populated; null otherwise.
    pub next: *mut lumpinfo_t,
}

/// Lump directory state (C `w_wad.c` globals).
#[derive(Clone, Copy)]
pub struct WadState {
    /// Pointer to the global lump directory. Mirrors the `lumpinfo` C
    /// global; sized by `numlumps`. Reallocated by `ExtendLumpInfo`
    /// every time a new file is added. Other modules reach it through
    /// [`lumpinfo`].
    lumpinfo: *mut lumpinfo_t,
    /// Number of entries in `lumpinfo`. Mirrors the C `numlumps` global.
    numlumps: u32,
    /// Hash table: `numlumps` buckets, each a singly-linked list through
    /// `lumpinfo_t::next`. Built lazily by `W_GenerateHashTable` and
    /// dropped whenever a new file is added.
    lumphash: *mut *mut lumpinfo_t,
}

impl WadState {
    /// No lumps loaded.
    pub const fn new() -> Self {
        Self {
            lumpinfo: ptr::null_mut(),
            numlumps: 0,
            lumphash: ptr::null_mut(),
        }
    }
}

impl Default for WadState {
    fn default() -> Self {
        Self::new()
    }
}

/// The lump directory state.
pub static WAD: Global<WadState> = Global::new(WadState::new());

/// The global lump directory (C `lumpinfo`), [`W_NumLumps`] entries long.
pub fn lumpinfo() -> *mut lumpinfo_t {
    WAD.get().lumpinfo
}

/// Allocation layout of a `lumpinfo` array of `count` entries. At least
/// one entry is reserved, because the allocator does not take zero-sized
/// layouts.
fn lumpinfo_layout(count: u32) -> std::alloc::Layout {
    std::alloc::Layout::array::<lumpinfo_t>(count.max(1) as usize).unwrap()
}

/// Grow the global `lumpinfo` array to `newnumlumps` entries, copy
/// the existing entries across, fix up any zone-allocator user
/// pointers (`Z_ChangeUser`), and re-link any in-flight `next`
/// chains so they point into the new array.
///
/// On allocation failure, calls `I_Error`. After return,
/// `lumpinfo` points at the new array, `numlumps == newnumlumps`,
/// and the old array has been freed. The arrays are allocated zeroed, as
/// the C `calloc` did, with [`lumpinfo_layout`].
///
/// # Safety
///
/// Mutates the `lumpinfo` and `numlumps` globals. Assumes
/// `newnumlumps >= numlumps` and that `lumpinfo` is null or was
/// allocated here for `numlumps` entries. Mirrors the file-static helper
/// of the same name in `w_wad.c`.
unsafe fn ExtendLumpInfo(newnumlumps: u32) {
    let WadState {
        lumpinfo, numlumps, ..
    } = WAD.get();
    let newlumpinfo = std::alloc::alloc_zeroed(lumpinfo_layout(newnumlumps)) as *mut lumpinfo_t;
    if newlumpinfo.is_null() {
        i_error!("Couldn't realloc lumpinfo");
    }

    for i in 0..numlumps.min(newnumlumps) {
        std::ptr::copy_nonoverlapping(lumpinfo.add(i as usize), newlumpinfo.add(i as usize), 1);

        if !(*newlumpinfo.add(i as usize)).cache.is_null() {
            Z_ChangeUser(
                (*newlumpinfo.add(i as usize)).cache,
                &mut (*newlumpinfo.add(i as usize)).cache as *mut *mut c_void,
            );
        }

        if !(*lumpinfo.add(i as usize)).next.is_null() {
            let nextlumpnum = ((*lumpinfo.add(i as usize)).next as usize - lumpinfo as usize)
                / std::mem::size_of::<lumpinfo_t>();
            (*newlumpinfo.add(i as usize)).next = newlumpinfo.add(nextlumpnum);
        }
    }

    if !lumpinfo.is_null() {
        std::alloc::dealloc(lumpinfo as *mut u8, lumpinfo_layout(numlumps));
    }
    let mut wad = WAD.borrow_mut();
    wad.lumpinfo = newlumpinfo;
    wad.numlumps = newnumlumps;
}

/// The significant part of a lump name: at most 8 bytes, ending at
/// the first NUL.
///
/// Lump names are stored as 8 NUL-padded bytes with no terminator
/// when full, so every comparison and hash works on this prefix,
/// matching the C code's `strncasecmp(a, b, 8)`.
pub fn lump_name(name: &[u8]) -> &[u8] {
    until_nul(&name[..name.len().min(8)])
}

/// Case-insensitive lump-name equality over the [`lump_name`] prefixes.
pub fn lump_names_equal(first: &[u8], second: &[u8]) -> bool {
    lump_name(first).eq_ignore_ascii_case(lump_name(second))
}

/// djb2-style hash of a lump name.
///
/// The hash uses `((h << 5) ^ h) ^ toupper(ch)` over the
/// [`lump_name`] prefix, so the result is case-insensitive and
/// matches the C reference in `w_wad.c` exactly.
pub fn W_LumpNameHash(name: &[u8]) -> u32 {
    lump_name(name).iter().fold(5381, |hash: u32, &byte| {
        ((hash << 5) ^ hash) ^ u32::from(byte.to_ascii_uppercase())
    })
}

/// Open `filename` and append its lumps to the global directory.
///
/// Files whose extension is not `wad` (case-insensitive) are loaded
/// as single-lump files: a synthetic `filelump_t` is built whose
/// name is the file's basename (via `M_ExtractFileBase`). True WAD
/// files read the 12-byte `wadinfo_t` header, validate the `IWAD`
/// or `PWAD` magic, and then load the full directory at
/// `infotableofs`. Both are read as the little-endian records of
/// [`format`].
///
/// On success returns the borrowed `wad_file_t*`, owned by
/// `w_file`. On open failure prints `couldn't open <path>` and
/// returns null. Any existing `lumphash` is freed so the next
/// lookup falls back to the linear scan until
/// `W_GenerateHashTable` is called again.
pub fn W_AddFile(filename: &str) -> *mut wad_file_t {
    unsafe {
        let wad_file = W_OpenFile(filename);
        if wad_file.is_null() {
            println!(" couldn't open {filename}");
            return ptr::null_mut();
        }

        let startlump = WAD.get().numlumps;

        let path = filename.as_bytes();
        let fileinfo = if path.len() < 3 || !path[path.len() - 3..].eq_ignore_ascii_case(b"wad") {
            // Single lump file
            vec![filelump_t {
                filepos: I32::ZERO,
                size: I32::new((*wad_file).length as i32),
                name: M_ExtractFileBase(path),
            }]
        } else {
            // WAD file
            let mut header = wadinfo_t::new_zeroed();
            let header_bytes = header.as_mut_bytes();
            W_Read(
                wad_file,
                0,
                header_bytes.as_mut_ptr() as *mut c_void,
                header_bytes.len(),
            );

            if header.identification != *b"IWAD" && header.identification != *b"PWAD" {
                i_error!("Wad file {filename} doesn't have IWAD or PWAD id");
            }

            let mut directory = vec![filelump_t::new_zeroed(); header.numlumps.get() as usize];
            let directory_bytes = directory.as_mut_bytes();
            W_Read(
                wad_file,
                header.infotableofs.get() as u32,
                directory_bytes.as_mut_ptr() as *mut c_void,
                directory_bytes.len(),
            );
            directory
        };

        ExtendLumpInfo(startlump + fileinfo.len() as u32);

        let mut lump_p = lumpinfo().add(startlump as usize);

        for filelump in &fileinfo {
            (*lump_p).wad_file = wad_file;
            (*lump_p).position = filelump.filepos.get();
            (*lump_p).size = filelump.size.get();
            (*lump_p).cache = ptr::null_mut();
            // strncpy semantics: copy up to the first NUL, zero-pad the rest.
            let name = lump_name(&filelump.name);
            let mut padded = [0; 8];
            padded[..name.len()].copy_from_slice(name);
            (*lump_p).name = padded;

            lump_p = lump_p.add(1);
        }

        let lumphash = WAD.get().lumphash;
        if !lumphash.is_null() {
            Z_Free(lumphash as *mut c_void);
            WAD.borrow_mut().lumphash = ptr::null_mut();
        }

        wad_file
    }
}

/// Return the total number of registered lumps as a signed integer.
/// Mirrors `W_NumLumps` from `w_wad.c`.
pub fn W_NumLumps() -> i32 {
    WAD.get().numlumps as i32
}

/// Look up a lump by name and return its index, or `-1` if not
/// found.
///
/// Uses the hash table when `W_GenerateHashTable` has been called;
/// otherwise scans `lumpinfo` backwards so that later-loaded WADs
/// override earlier ones (matching the C implementation).
/// Names compare case-insensitively via [`lump_names_equal`].
pub fn W_CheckNumForName(name: &[u8]) -> i32 {
    let WadState {
        lumpinfo,
        numlumps,
        lumphash,
    } = WAD.get();
    unsafe {
        if !lumphash.is_null() {
            let hash = (W_LumpNameHash(name) % numlumps) as usize;
            let mut lump_p = *lumphash.add(hash);
            while !lump_p.is_null() {
                if lump_names_equal(&(*lump_p).name, name) {
                    return (lump_p as usize - lumpinfo as usize) as i32
                        / std::mem::size_of::<lumpinfo_t>() as i32;
                }
                lump_p = (*lump_p).next;
            }
        } else {
            // Linear search, backwards so patch lumps take precedence
            let mut i = numlumps as i32 - 1;
            while i >= 0 {
                if lump_names_equal(&(*lumpinfo.add(i as usize)).name, name) {
                    return i;
                }
                i -= 1;
            }
        }
        -1
    }
}

/// Look up a lump by name and return its index. Calls `I_Error`
/// (and never returns) if the lump is missing. The C source uses
/// this as the strict variant; callers that tolerate misses use
/// `W_CheckNumForName` directly.
pub fn W_GetNumForName(name: &[u8]) -> i32 {
    let i = W_CheckNumForName(name);
    if i < 0 {
        i_error!(
            "W_GetNumForName: {} not found!",
            String::from_utf8_lossy(lump_name(name))
        );
    }
    i
}

/// Return the byte size of `lump`. Errors out via `I_Error` if
/// `lump` is out of range. Mirrors `W_LumpLength` from `w_wad.c`.
pub fn W_LumpLength(lump: u32) -> i32 {
    let WadState {
        lumpinfo, numlumps, ..
    } = WAD.get();
    unsafe {
        if lump >= numlumps {
            i_error!("W_LumpLength: {} >= numlumps", lump as i32);
        }
        (*lumpinfo.add(lump as usize)).size
    }
}

/// Read `lump` into the caller-supplied buffer `dest`.
///
/// Wraps the read in `I_BeginRead` / `I_EndRead` so the platform can
/// display a disk icon. If the underlying `W_Read` returns fewer
/// bytes than requested, calls `I_Error`.
///
/// Differs from the C source by also emitting a stderr diagnostic
/// when the actual read exceeds `lump.size` (a "shouldn't happen"
/// defensive log, not present in `w_wad.c`).
///
/// # Safety
/// `dest` must be writable for at least `W_LumpLength(lump)` bytes.
pub unsafe fn W_ReadLump(lump: u32, dest: *mut c_void) {
    let WadState {
        lumpinfo, numlumps, ..
    } = WAD.get();
    unsafe {
        if lump >= numlumps {
            i_error!("W_ReadLump: {} >= numlumps", lump as i32);
        }
        let l = lumpinfo.add(lump as usize);
        I_BeginRead();
        let c = W_Read(
            (*l).wad_file,
            (*l).position as u32,
            dest,
            (*l).size as usize,
        );
        if c > (*l).size as usize {
            eprintln!(
                "[W_ReadLump] OVERFLOW: lump={}, requested={}, actually_read={}, lump.size={}",
                lump,
                (*l).size,
                c,
                (*l).size
            );
        }
        if c < (*l).size as usize {
            i_error!(
                "W_ReadLump: only read {} of {} on lump {}",
                c as i32,
                (*l).size,
                lump as i32
            );
        }
        I_EndRead();
    }
}

/// Return a borrowed pointer to the bytes of `lumpnum`, loading
/// them into the zone cache on first miss.
///
/// Three branches:
///  - Memory-mapped wad: returns a pointer inside the mapping (no
///    copy, no zone allocation).
///  - Already cached: returns the cached pointer and switches its
///    zone tag to `tag` via `Z_ChangeTag2`.
///  - Cold miss: `Z_Malloc(size, tag, &cache)` and `W_ReadLump`.
///
/// `tag` is typically `PU_STATIC` (long-lived) or `PU_CACHE`
/// (purgeable). Out-of-range `lumpnum` triggers `I_Error`.
pub fn W_CacheLumpNum(lumpnum: i32, tag: i32) -> *mut c_void {
    let WadState {
        lumpinfo, numlumps, ..
    } = WAD.get();
    unsafe {
        if lumpnum < 0 || (lumpnum as u32) >= numlumps {
            i_error!("W_CacheLumpNum: {} >= numlumps", lumpnum);
        }
        let lump = lumpinfo.add(lumpnum as usize);

        if !(*(*lump).wad_file).mapped.is_null() {
            // Memory-mapped file
            (*(*lump).wad_file).mapped.add((*lump).position as usize) as *mut c_void
        } else if !(*lump).cache.is_null() {
            // Already cached
            let result = (*lump).cache;
            Z_ChangeTag2(result, tag);
            result
        } else {
            // Not yet loaded
            (*lump).cache = Z_Malloc(
                W_LumpLength(lumpnum as u32),
                tag,
                &mut (*lump).cache as *mut *mut c_void as *mut c_void,
            );
            W_ReadLump(lumpnum as u32, (*lump).cache);
            (*lump).cache
        }
    }
}

/// Convenience wrapper: resolve `name` to a lump number via
/// `W_GetNumForName` (fatal if missing) and call `W_CacheLumpNum`.
pub fn W_CacheLumpName(name: &[u8], tag: i32) -> *mut c_void {
    W_CacheLumpNum(W_GetNumForName(name), tag)
}

/// Cache lump `lumpnum` with `W_CacheLumpNum` and view its payload as
/// bytes.
///
/// # Safety
///
/// The slice borrows the lump cache: it is valid only until the lump
/// is released with `W_ReleaseLumpNum` or, at `PU_CACHE`, purged.
pub unsafe fn cache_lump_bytes<'a>(lumpnum: i32, tag: i32) -> &'a [u8] {
    let data = W_CacheLumpNum(lumpnum, tag) as *const u8;
    std::slice::from_raw_parts(data, W_LumpLength(lumpnum as u32) as usize)
}

/// Cache lump `lumpnum` with `W_CacheLumpNum` and view it as a [`Patch`].
/// A lump that is not a patch is a fatal `I_Error`, naming the lump.
///
/// # Safety
///
/// The view borrows the lump cache, as for [`cache_lump_bytes`], so the
/// caller picks `'a` no longer than the lump stays cached:
///
/// - At `PU_CACHE` the zone may purge the lump on any later `Z_Malloc`,
///   including a `W_CacheLumpNum` miss for another lump, so use the view
///   only until the next zone allocation.
/// - At `PU_STATIC` (or `PU_LEVEL`) the lump stays until it is released
///   with `W_ReleaseLumpNum`, freed with its tag, or cached again at a
///   purgeable tag (`W_CacheLumpNum` re-tags a cached lump), so a
///   `'static` view needs every such release to drop the view first.
pub unsafe fn cache_patch_num<'a>(lumpnum: i32, tag: i32) -> Patch<'a> {
    let bytes = cache_lump_bytes(lumpnum, tag);
    match Patch::from_bytes(bytes) {
        Ok(patch) => patch,
        Err(error) => i_error!(
            "cache_patch_num: lump {} ({}) is not a patch: {}",
            lumpnum,
            String::from_utf8_lossy(lump_name(&(*lumpinfo().add(lumpnum as usize)).name)),
            error
        ),
    }
}

/// Resolve `name` with `W_GetNumForName` (fatal if missing) and call
/// [`cache_patch_num`].
///
/// # Safety
///
/// As for [`cache_patch_num`].
pub unsafe fn cache_patch_name<'a>(name: &[u8], tag: i32) -> Patch<'a> {
    cache_patch_num(W_GetNumForName(name), tag)
}

/// Cache lump `lumpnum` at `PU_STATIC` and view it as an array of
/// on-disk records from [`format`]. A partial record at the end is
/// ignored, as the C count `W_LumpLength / sizeof(record)` did.
///
/// # Safety
///
/// As for [`cache_lump_bytes`]: release the lump only after the last
/// use of the slice.
pub unsafe fn cache_lump_records<'a, Record>(lumpnum: i32) -> &'a [Record]
where
    Record: FromBytes + Immutable + KnownLayout,
{
    let bytes = cache_lump_bytes(lumpnum, PU_STATIC);
    let count = bytes.len() / std::mem::size_of::<Record>();
    match <[Record]>::ref_from_prefix_with_elems(bytes, count) {
        Ok((records, _partial_record)) => records,
        Err(_) => i_error!(
            "cache_lump_records: lump {} is not an array of {}-byte records",
            lumpnum,
            std::mem::size_of::<Record>()
        ),
    }
}

/// Mark `lumpnum` as releasable by demoting its cached block to
/// `PU_CACHE`, so the zone allocator may purge it under memory
/// pressure. No-op for memory-mapped wads.
///
/// Mirrors `W_ReleaseLumpNum` from `w_wad.c`.
pub fn W_ReleaseLumpNum(lumpnum: i32) {
    let WadState {
        lumpinfo, numlumps, ..
    } = WAD.get();
    unsafe {
        if lumpnum < 0 || (lumpnum as u32) >= numlumps {
            i_error!("W_ReleaseLumpNum: {} >= numlumps", lumpnum);
        }
        let lump = lumpinfo.add(lumpnum as usize);
        if !(*(*lump).wad_file).mapped.is_null() {
            // Memory-mapped: nothing to do
        } else {
            Z_ChangeTag2((*lump).cache, PU_CACHE);
        }
    }
}

/// Convenience wrapper: resolve `name` and call `W_ReleaseLumpNum`.
/// Mirrors `W_ReleaseLumpName` from `w_wad.c`.
pub fn W_ReleaseLumpName(name: &[u8]) {
    W_ReleaseLumpNum(W_GetNumForName(name))
}

/// Build the lump-name hash table. Allocates `numlumps` buckets in
/// the zone heap, then inserts every lump into the bucket given by
/// `W_LumpNameHash(name) % numlumps`, chaining through
/// `lumpinfo_t::next`. Frees any pre-existing table first.
///
/// Called once after the last `W_AddFile`; subsequent additions
/// invalidate (free) the table so a regeneration must be requested
/// explicitly.
pub fn W_GenerateHashTable() {
    let WadState {
        lumpinfo,
        numlumps,
        lumphash,
    } = WAD.get();
    unsafe {
        if !lumphash.is_null() {
            Z_Free(lumphash as *mut c_void);
        }

        if numlumps > 0 {
            let lumphash = Z_Malloc(
                (std::mem::size_of::<*mut lumpinfo_t>() * numlumps as usize) as i32,
                PU_STATIC,
                ptr::null_mut(),
            ) as *mut *mut lumpinfo_t;
            std::ptr::write_bytes(lumphash, 0, numlumps as usize);

            for i in 0..numlumps {
                let hash = (W_LumpNameHash(&(*lumpinfo.add(i as usize)).name) % numlumps) as usize;
                (*lumpinfo.add(i as usize)).next = *lumphash.add(hash);
                *lumphash.add(hash) = lumpinfo.add(i as usize);
            }
            WAD.borrow_mut().lumphash = lumphash;
        }
    }
}

/// Refuse to launch when the user supplies an IWAD whose unique
/// marker lump belongs to a different game (e.g. running with
/// `hexen.wad` while the active mission is Doom).
///
/// For each `(mission, lumpname)` pair in `UNIQUE_LUMPS`, if the
/// active `mission` differs but the lump is still present, call
/// `I_Error` with a friendly message suggesting the right binary.
///
/// The C source uses a `PROGRAM_PREFIX` macro for the binary name;
/// this port hardcodes `"doomgeneric"`.
pub fn W_CheckCorrectIWAD(mission: i32) {
    /// One row of the IWAD-mismatch detection table.
    struct UniqueLump {
        /// `GameMission_t` value for which this lump is expected.
        mission: i32,
        /// Lump name that uniquely identifies the mission's IWAD.
        lumpname: &'static [u8],
    }

    /// Table of lumps that uniquely identify each supported IWAD.
    /// Order/contents mirror the `unique_lumps[]` array in `w_wad.c`.
    const UNIQUE_LUMPS: [UniqueLump; 4] = [
        UniqueLump {
            mission: 0, // doom
            lumpname: b"POSSA1",
        },
        UniqueLump {
            mission: 6, // heretic
            lumpname: b"IMPXA1",
        },
        UniqueLump {
            mission: 7, // hexen
            lumpname: b"ETTNA1",
        },
        UniqueLump {
            mission: 8, // strife
            lumpname: b"AGRDA1",
        },
    ];

    for ul in &UNIQUE_LUMPS {
        if mission != ul.mission {
            let lumpnum = W_CheckNumForName(ul.lumpname);
            if lumpnum >= 0 {
                i_error!(
                    "\nYou are trying to use a {} IWAD file with the {}{}binary.\nThis isn't going to work.\nYou probably want to use the {}{}binary.",
                    D_SuggestGameName(ul.mission, 4),
                    "doomgeneric",
                    D_GameMissionString(mission),
                    "doomgeneric",
                    D_GameMissionString(ul.mission)
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::sync::Mutex};

    /// Mutex serialising tests that swap out the global
    /// `lumpinfo` / `numlumps` / `lumphash` state. Cargo runs tests
    /// in parallel by default; without this guard two tests could
    /// see each other's fake globals.
    static WAD_LOCK: Mutex<()> = Mutex::new(());

    /// RAII scope that installs fake values for the WAD globals on
    /// construction and restores the originals on drop. Declared
    /// **before** the local backing storage in each test so it
    /// drops first (LIFO), restoring the globals before the storage
    /// itself goes out of scope.
    struct WadTestScope {
        /// Original lump directory state to restore on drop.
        saved: WadState,
    }

    impl WadTestScope {
        /// Save the current globals and install `info` /`count` /
        /// `null` for the duration of the test. Returns the guard.
        ///
        /// # Safety
        ///
        /// `info` must remain live for the lifetime of the returned
        /// guard. The caller must serialise with `WAD_LOCK`.
        unsafe fn install(info: *mut lumpinfo_t, count: u32) -> Self {
            let scope = WadTestScope { saved: WAD.get() };
            WAD.set(WadState {
                lumpinfo: info,
                numlumps: count,
                lumphash: ptr::null_mut(), // force linear scan, not hash table
            });
            scope
        }
    }

    /// RAII restoration of the saved WAD globals on test teardown.
    impl Drop for WadTestScope {
        /// Restore the saved globals.
        fn drop(&mut self) {
            WAD.set(self.saved);
        }
    }

    /// Build a `[u8; 8]` lump name from a byte slice, NUL-padding
    /// or truncating to 8 bytes.
    fn make_lump_name(s: &[u8]) -> [u8; 8] {
        let mut name = [0u8; 8];
        let len = s.len().min(8);
        name[..len].copy_from_slice(&s[..len]);
        name
    }

    /// Build a fake `wad_file_t` whose `mapped` pointer is non-null,
    /// so `W_ReleaseLumpNum` takes the memory-mapped no-op branch
    /// and avoids the zone allocator (which is not initialised in
    /// the test harness).
    fn mapped_wad() -> wad_file_t {
        static SENTINEL: u8 = 0;
        wad_file_t::mapped_for_test(std::ptr::addr_of!(SENTINEL).cast_mut())
    }

    /// Build a minimal `lumpinfo_t` for tests: just the name, the
    /// owning fake wad, and zero/null everywhere else.
    fn make_lump(name: &[u8], wad: *mut wad_file_t) -> lumpinfo_t {
        lumpinfo_t {
            name: make_lump_name(name),
            wad_file: wad,
            position: 0,
            size: 0,
            cache: ptr::null_mut(),
            next: ptr::null_mut(),
        }
    }

    /// `W_ReleaseLumpName` must delegate correctly: the name
    /// resolves to lump 0 and `W_ReleaseLumpNum(0)` completes
    /// without error.
    #[test]
    fn release_lump_name_delegates_to_num() {
        let _lock = WAD_LOCK.lock().unwrap();
        let mut wad = mapped_wad();
        let mut lumps = [make_lump(b"TESTLUMP", &mut wad)];
        let _scope = unsafe { WadTestScope::install(lumps.as_mut_ptr(), 1) };

        W_ReleaseLumpName(b"TESTLUMP");
    }

    /// Names are compared over their first 8 bytes up to a NUL, so a
    /// full unterminated name, a padded name and a longer buffer agree.
    #[test]
    fn lump_names_compare_like_strncasecmp_8() {
        assert_eq!(lump_name(b"POSSA1\0\0"), b"POSSA1");
        assert_eq!(lump_name(b"TESTLUMPX"), b"TESTLUMP");
        assert_eq!(lump_name(b"AB\0CD"), b"AB");
        assert!(lump_names_equal(b"possa1", b"POSSA1\0\0"));
        assert!(lump_names_equal(b"TESTLUMP", b"testlumpextra"));
        assert!(!lump_names_equal(b"POSSA", b"POSSA1"));
        assert_eq!(W_LumpNameHash(b"possa1"), W_LumpNameHash(b"POSSA1\0junk"));
        // djb2-xor over "A": ((5381 << 5) ^ 5381) ^ 'A'
        assert_eq!(
            W_LumpNameHash(b"a"),
            ((5381u32 << 5) ^ 5381) ^ u32::from(b'A')
        );
    }

    /// The underlying `strncasecmp` lookup is case-insensitive, so
    /// "testlump" must resolve to the same entry as "TESTLUMP".
    #[test]
    fn release_lump_name_is_case_insensitive() {
        let _lock = WAD_LOCK.lock().unwrap();
        let mut wad = mapped_wad();
        let mut lumps = [make_lump(b"TESTLUMP", &mut wad)];
        let _scope = unsafe { WadTestScope::install(lumps.as_mut_ptr(), 1) };

        W_ReleaseLumpName(b"testlump");
    }

    /// With multiple lumps loaded, each name must resolve to its
    /// own entry. The linear scan runs backwards so the
    /// last-registered match wins for duplicates - here every name
    /// is unique so order is immaterial.
    #[test]
    fn release_lump_name_picks_correct_lump_among_multiple() {
        let _lock = WAD_LOCK.lock().unwrap();
        let mut wad = mapped_wad();
        let mut lumps = [make_lump(b"ALPHA", &mut wad), make_lump(b"BETA", &mut wad)];
        let _scope = unsafe { WadTestScope::install(lumps.as_mut_ptr(), 2) };

        W_ReleaseLumpName(b"ALPHA");
        W_ReleaseLumpName(b"BETA");
    }
}
