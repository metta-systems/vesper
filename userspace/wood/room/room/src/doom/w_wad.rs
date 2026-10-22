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

use crate::i_error;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::ptr;

use crate::doom::w_file::{wad_file_t, W_OpenFile, W_Read};

use crate::doom::d_iwad::D_SuggestGameName;
use crate::doom::d_mode::D_GameMissionString;
use crate::doom::i_video::{I_BeginRead, I_EndRead};
use crate::doom::m_misc::M_ExtractFileBase;
use crate::doom::z_zone::{Z_ChangeTag2, Z_ChangeUser, Z_Free, Z_Malloc, PU_CACHE, PU_STATIC};

/// One entry in the global lump directory. Mirrors `lumpinfo_t` in
/// `w_wad.h`. The size (40 bytes on x86_64) is asserted by a
/// `cfg(test)` test below since other modules read this layout
/// across the FFI boundary.
#[repr(C)]
pub struct lumpinfo_t {
    /// 8-char ASCII lump name, **not** NUL-terminated when full.
    pub name: [c_char; 8],
    /// File the lump lives in.
    pub wad_file: *mut wad_file_t,
    /// Offset of the lump payload inside the file, in bytes.
    pub position: c_int,
    /// Payload size in bytes.
    pub size: c_int,
    /// Zone-allocated cache pointer, or null if not yet loaded.
    /// Memory-mapped files leave this null; `W_CacheLumpNum`
    /// returns a pointer into the mapping directly.
    pub cache: *mut c_void,
    /// Next entry in the per-hash-bucket chain when `lumphash` is
    /// populated; null otherwise.
    pub next: *mut lumpinfo_t,
}

/// On-disk WAD header. Read from offset 0 of every `.wad` file
/// loaded by `W_AddFile`. Layout matches `wadinfo_t` in `w_wad.c`,
/// 12 bytes on x86_64.
#[repr(C)]
struct wadinfo_t {
    /// Magic identifier: `"IWAD"` for the main IWAD, `"PWAD"` for
    /// a patch WAD. Anything else triggers `I_Error`.
    identification: [c_char; 4],
    /// Little-endian number of lumps in the directory.
    numlumps: c_int,
    /// Little-endian byte offset to the lump-directory table.
    infotableofs: c_int,
}

/// On-disk directory entry as it appears at `infotableofs`. Mirrors
/// `filelump_t` in `w_wad.c`, 16 bytes on x86_64.
#[repr(C)]
struct filelump_t {
    /// Little-endian byte offset of the lump payload inside the WAD.
    filepos: c_int,
    /// Little-endian payload length in bytes.
    size: c_int,
    /// 8-char ASCII lump name, NUL-padded.
    name: [c_char; 8],
}

/// Pointer to the global lump directory. Mirrors the `lumpinfo` C
/// global; sized by `numlumps`. Reallocated by `ExtendLumpInfo`
/// every time a new file is added. C linkage so other translation
/// units (and tests) can reach it.
#[no_mangle]
pub static mut lumpinfo: *mut lumpinfo_t = ptr::null_mut();

/// Number of entries in `lumpinfo`. Mirrors the C `numlumps` global.
#[no_mangle]
pub static mut numlumps: c_uint = 0;

/// Hash table: `numlumps` buckets, each a singly-linked list through
/// `lumpinfo_t::next`. Built lazily by `W_GenerateHashTable` and
/// dropped whenever a new file is added.
static mut lumphash: *mut *mut lumpinfo_t = ptr::null_mut();

extern "C" {
    /// libc: case-insensitive compare of first `n` bytes.
    fn strncasecmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    /// libc: case-insensitive string compare.
    fn strcasecmp(s1: *const c_char, s2: *const c_char) -> c_int;
    /// libc: byte-equal compare of first `n` bytes.
    fn strncmp(s1: *const c_char, s2: *const c_char, n: usize) -> c_int;
    /// libc: copy up to `n` bytes, NUL-padding the destination.
    fn strncpy(dst: *mut c_char, src: *const c_char, n: usize) -> *mut c_char;
    /// libc: NUL-terminated string length.
    fn strlen(s: *const c_char) -> usize;
    /// libc: ASCII-upper-case.
    fn toupper(c: c_int) -> c_int;

    /// libc: zero-initialised allocation.
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
    /// libc: free a `malloc`/`calloc` block.
    fn free(ptr: *mut c_void);
}

/// Grow the global `lumpinfo` array to `newnumlumps` entries, copy
/// the existing entries across, fix up any zone-allocator user
/// pointers (`Z_ChangeUser`), and re-link any in-flight `next`
/// chains so they point into the new array.
///
/// On allocation failure, calls `I_Error`. After return,
/// `lumpinfo` points at the new array, `numlumps == newnumlumps`,
/// and the old array has been `free`'d.
///
/// # Safety
///
/// Mutates the `lumpinfo` and `numlumps` globals. Assumes
/// `newnumlumps >= numlumps`. Mirrors the file-static helper of
/// the same name in `w_wad.c`.
unsafe fn ExtendLumpInfo(newnumlumps: c_uint) {
    let newlumpinfo =
        calloc(newnumlumps as usize, std::mem::size_of::<lumpinfo_t>()) as *mut lumpinfo_t;
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

    free(lumpinfo as *mut c_void);
    lumpinfo = newlumpinfo;
    numlumps = newnumlumps;
}

/// djb2-style hash of an 8-char lump name (NUL-terminated or padded).
///
/// The hash uses `((h << 5) ^ h) ^ toupper(ch)` per character, so
/// the result is case-insensitive and matches the C reference in
/// `w_wad.c` exactly. Caller must ensure `s` points to at least 8
/// bytes (or a shorter NUL-terminated string).
#[no_mangle]
pub extern "C" fn W_LumpNameHash(s: *const c_char) -> c_uint {
    unsafe {
        let mut result: c_uint = 5381;
        for i in 0..8 {
            let ch = *s.add(i);
            if ch == 0 {
                break;
            }
            result = ((result << 5) ^ result) ^ (toupper(ch as c_int) as c_uint);
        }
        result
    }
}

/// Open `filename` and append its lumps to the global directory.
///
/// Files whose extension is not `wad` (case-insensitive) are loaded
/// as single-lump files: a synthetic `filelump_t` is built whose
/// name is the file's basename (via `M_ExtractFileBase`). True WAD
/// files read the 12-byte `wadinfo_t` header, validate the `IWAD`
/// or `PWAD` magic, and then load the full directory at
/// `infotableofs`. Little-endian fields are byte-swapped via
/// `i32::from_le`.
///
/// On success returns the borrowed `wad_file_t*`, owned by
/// `w_file`. On open failure prints `couldn't open <path>` and
/// returns null. Any existing `lumphash` is freed so the next
/// lookup falls back to the linear scan until
/// `W_GenerateHashTable` is called again.
#[no_mangle]
pub extern "C" fn W_AddFile(filename: *mut c_char) -> *mut wad_file_t {
    unsafe {
        let wad_file = W_OpenFile(filename);
        if wad_file.is_null() {
            libc::printf(c" couldn't open %s\n".as_ptr(), filename);
            return ptr::null_mut();
        }

        let mut newnumlumps = numlumps;
        let startlump = numlumps;

        let fileinfo: *mut filelump_t;

        let fname_len = strlen(filename);
        if fname_len < 3 || strcasecmp(filename.add(fname_len - 3), c"wad".as_ptr()) != 0 {
            // Single lump file
            fileinfo = Z_Malloc(
                std::mem::size_of::<filelump_t>() as c_int,
                PU_STATIC,
                ptr::null_mut(),
            ) as *mut filelump_t;
            (*fileinfo).filepos = 0;
            (*fileinfo).size = (*wad_file).length as c_int;
            M_ExtractFileBase(filename, (*fileinfo).name.as_mut_ptr());
            newnumlumps += 1;
        } else {
            // WAD file
            let mut header: wadinfo_t = std::mem::zeroed();
            W_Read(
                wad_file,
                0,
                &mut header as *mut _ as *mut c_void,
                std::mem::size_of::<wadinfo_t>(),
            );

            if strncmp(header.identification.as_ptr(), c"IWAD".as_ptr(), 4) != 0
                && strncmp(header.identification.as_ptr(), c"PWAD".as_ptr(), 4) != 0
            {
                i_error!(
                    "Wad file {} doesn't have IWAD or PWAD id",
                    CStr::from_ptr(filename).to_string_lossy()
                );
            }

            let header_numlumps = i32::from_le(header.numlumps);
            let header_infotableofs = i32::from_le(header.infotableofs);

            let length = (header_numlumps as usize) * std::mem::size_of::<filelump_t>();
            fileinfo = Z_Malloc(length as c_int, PU_STATIC, ptr::null_mut()) as *mut filelump_t;

            W_Read(
                wad_file,
                header_infotableofs as c_uint,
                fileinfo as *mut c_void,
                length,
            );
            newnumlumps += header_numlumps as c_uint;
        }

        ExtendLumpInfo(newnumlumps);

        let mut lump_p = lumpinfo.add(startlump as usize);
        let mut filerover = fileinfo;

        for _i in startlump..numlumps {
            (*lump_p).wad_file = wad_file;
            (*lump_p).position = i32::from_le((*filerover).filepos);
            (*lump_p).size = i32::from_le((*filerover).size);
            (*lump_p).cache = ptr::null_mut();
            strncpy((*lump_p).name.as_mut_ptr(), (*filerover).name.as_ptr(), 8);

            lump_p = lump_p.add(1);
            filerover = filerover.add(1);
        }

        Z_Free(fileinfo as *mut c_void);

        if !lumphash.is_null() {
            Z_Free(lumphash as *mut c_void);
            lumphash = ptr::null_mut();
        }

        wad_file
    }
}

/// Return the total number of registered lumps as a signed integer.
/// Mirrors `W_NumLumps` from `w_wad.c`.
#[no_mangle]
pub extern "C" fn W_NumLumps() -> c_int {
    unsafe { numlumps as c_int }
}

/// Look up a lump by name and return its index, or `-1` if not
/// found.
///
/// Uses the hash table when `W_GenerateHashTable` has been called;
/// otherwise scans `lumpinfo` backwards so that later-loaded WADs
/// override earlier ones (matching the C implementation).
/// Comparison is via `strncasecmp` over 8 bytes, so trailing bytes
/// past a NUL must match too (they're zeroed in `lumpinfo_t::name`
/// after `strncpy`).
#[no_mangle]
pub extern "C" fn W_CheckNumForName(name: *const c_char) -> c_int {
    unsafe {
        if !lumphash.is_null() {
            let hash = (W_LumpNameHash(name) % numlumps) as usize;
            let mut lump_p = *lumphash.add(hash);
            while !lump_p.is_null() {
                if strncasecmp((*lump_p).name.as_ptr(), name, 8) == 0 {
                    return (lump_p as usize - lumpinfo as usize) as c_int
                        / std::mem::size_of::<lumpinfo_t>() as c_int;
                }
                lump_p = (*lump_p).next;
            }
        } else {
            // Linear search, backwards so patch lumps take precedence
            let mut i = numlumps as i32 - 1;
            while i >= 0 {
                if strncasecmp((*lumpinfo.add(i as usize)).name.as_ptr(), name, 8) == 0 {
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
#[no_mangle]
pub extern "C" fn W_GetNumForName(name: *const c_char) -> c_int {
    unsafe {
        let i = W_CheckNumForName(name);
        if i < 0 {
            i_error!(
                "W_GetNumForName: {} not found!",
                CStr::from_ptr(name).to_string_lossy()
            );
        }
        i
    }
}

/// Return the byte size of `lump`. Errors out via `I_Error` if
/// `lump` is out of range. Mirrors `W_LumpLength` from `w_wad.c`.
#[no_mangle]
pub extern "C" fn W_LumpLength(lump: c_uint) -> c_int {
    unsafe {
        if lump >= numlumps {
            i_error!("W_LumpLength: {} >= numlumps", lump as c_int);
        }
        (*lumpinfo.add(lump as usize)).size
    }
}

/// Read `lump` into the caller-supplied buffer `dest`.
///
/// The buffer must be at least `W_LumpLength(lump)` bytes. Wraps
/// the read in `I_BeginRead` / `I_EndRead` so the platform can
/// display a disk icon. If the underlying `W_Read` returns fewer
/// bytes than requested, calls `I_Error`.
///
/// Differs from the C source by also emitting a stderr diagnostic
/// when the actual read exceeds `lump.size` (a "shouldn't happen"
/// defensive log, not present in `w_wad.c`).
#[no_mangle]
pub extern "C" fn W_ReadLump(lump: c_uint, dest: *mut c_void) {
    unsafe {
        if lump >= numlumps {
            i_error!("W_ReadLump: {} >= numlumps", lump as c_int);
        }
        let l = lumpinfo.add(lump as usize);
        I_BeginRead();
        let c = W_Read(
            (*l).wad_file,
            (*l).position as c_uint,
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
                c as c_int,
                (*l).size,
                lump as c_int
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
#[no_mangle]
pub extern "C" fn W_CacheLumpNum(lumpnum: c_int, tag: c_int) -> *mut c_void {
    unsafe {
        if lumpnum < 0 || (lumpnum as c_uint) >= numlumps {
            i_error!("W_CacheLumpNum: {} >= numlumps", lumpnum);
        }
        let lump = lumpinfo.add(lumpnum as usize);

        if !(*(*lump).wad_file).mapped.is_null() {
            // Memory-mapped file
            (*(*lump).wad_file).mapped.add((*lump).position as usize) as *mut c_void
        } else if !(*lump).cache.is_null() {
            // Already cached
            let result = (*lump).cache;
            Z_ChangeTag2(result, tag, ptr::null(), 0);
            result
        } else {
            // Not yet loaded
            (*lump).cache = Z_Malloc(
                W_LumpLength(lumpnum as c_uint),
                tag,
                &mut (*lump).cache as *mut *mut c_void as *mut c_void,
            );
            W_ReadLump(lumpnum as c_uint, (*lump).cache);
            (*lump).cache
        }
    }
}

/// Convenience wrapper: resolve `name` to a lump number via
/// `W_GetNumForName` (fatal if missing) and call `W_CacheLumpNum`.
#[no_mangle]
pub extern "C" fn W_CacheLumpName(name: *const c_char, tag: c_int) -> *mut c_void {
    W_CacheLumpNum(W_GetNumForName(name), tag)
}

/// Mark `lumpnum` as releasable by demoting its cached block to
/// `PU_CACHE`, so the zone allocator may purge it under memory
/// pressure. No-op for memory-mapped wads.
///
/// Mirrors `W_ReleaseLumpNum` from `w_wad.c`.
#[no_mangle]
pub extern "C" fn W_ReleaseLumpNum(lumpnum: c_int) {
    unsafe {
        if lumpnum < 0 || (lumpnum as c_uint) >= numlumps {
            i_error!("W_ReleaseLumpNum: {} >= numlumps", lumpnum);
        }
        let lump = lumpinfo.add(lumpnum as usize);
        if !(*(*lump).wad_file).mapped.is_null() {
            // Memory-mapped: nothing to do
        } else {
            Z_ChangeTag2((*lump).cache, PU_CACHE, ptr::null(), 0);
        }
    }
}

/// Convenience wrapper: resolve `name` and call `W_ReleaseLumpNum`.
/// Mirrors `W_ReleaseLumpName` from `w_wad.c`.
#[no_mangle]
pub extern "C" fn W_ReleaseLumpName(name: *const c_char) {
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
#[no_mangle]
pub extern "C" fn W_GenerateHashTable() {
    unsafe {
        if !lumphash.is_null() {
            Z_Free(lumphash as *mut c_void);
        }

        if numlumps > 0 {
            lumphash = Z_Malloc(
                (std::mem::size_of::<*mut lumpinfo_t>() * numlumps as usize) as c_int,
                PU_STATIC,
                ptr::null_mut(),
            ) as *mut *mut lumpinfo_t;
            std::ptr::write_bytes(lumphash, 0, numlumps as usize);

            for i in 0..numlumps {
                let hash =
                    (W_LumpNameHash((*lumpinfo.add(i as usize)).name.as_ptr()) % numlumps) as usize;
                (*lumpinfo.add(i as usize)).next = *lumphash.add(hash);
                *lumphash.add(hash) = lumpinfo.add(i as usize);
            }
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
#[no_mangle]
pub extern "C" fn W_CheckCorrectIWAD(mission: c_int) {
    /// One row of the IWAD-mismatch detection table.
    struct UniqueLump {
        /// `GameMission_t` value for which this lump is expected.
        mission: c_int,
        /// 8-char lump name (NUL-padded) that uniquely identifies
        /// the mission's IWAD.
        lumpname: &'static [u8],
    }

    /// Table of lumps that uniquely identify each supported IWAD.
    /// Order/contents mirror the `unique_lumps[]` array in `w_wad.c`.
    const UNIQUE_LUMPS: [UniqueLump; 4] = [
        UniqueLump {
            mission: 0, // doom
            lumpname: b"POSSA1\0\0",
        },
        UniqueLump {
            mission: 6, // heretic
            lumpname: b"IMPXA1\0\0",
        },
        UniqueLump {
            mission: 7, // hexen
            lumpname: b"ETTNA1\0\0",
        },
        UniqueLump {
            mission: 8, // strife
            lumpname: b"AGRDA1\0\0",
        },
    ];

    unsafe {
        for ul in &UNIQUE_LUMPS {
            if mission != ul.mission {
                let lumpnum = W_CheckNumForName(ul.lumpname.as_ptr() as *mut c_char);
                if lumpnum >= 0 {
                    i_error!(
                        "\nYou are trying to use a {} IWAD file with the {}{}binary.\nThis isn't going to work.\nYou probably want to use the {}{}binary.",
                        CStr::from_ptr(D_SuggestGameName(ul.mission, 4)).to_string_lossy(),
                        "doomgeneric",
                        CStr::from_ptr(D_GameMissionString(mission)).to_string_lossy(),
                        "doomgeneric",
                        CStr::from_ptr(D_GameMissionString(ul.mission)).to_string_lossy()
                    );
                }
            }
        }
    }
}
/// Link anchor referencing every public C symbol in this module so
/// the linker keeps them all. Not part of the original Doom API.
///
/// # Safety
///
/// Passes null pointers everywhere and would crash if called.
/// Treat as link-only.
#[no_mangle]
pub unsafe extern "C" fn W_Wad_Link_Anchor() {
    W_LumpNameHash(ptr::null());
    W_AddFile(ptr::null_mut());
    W_NumLumps();
    W_CheckNumForName(ptr::null());
    W_GetNumForName(ptr::null());
    W_LumpLength(0);
    W_ReadLump(0, ptr::null_mut());
    W_CacheLumpNum(0, 0);
    W_CacheLumpName(ptr::null(), 0);
    W_ReleaseLumpNum(0);
    W_ReleaseLumpName(ptr::null());
    W_GenerateHashTable();
    W_CheckCorrectIWAD(0);
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `lumpinfo_t` is shared with C code: its 40-byte size on
    /// x86_64 must not silently change.
    #[test]
    fn lumpinfo_size_matches_c() {
        // C lumpinfo_t = name[8] + wad_file* + position + size + cache + next
        // On x86_64: 8 + 8 + 4 + 4 + 8 + 8 = 40 bytes
        assert_eq!(std::mem::size_of::<lumpinfo_t>(), 40);
    }

    /// `wadinfo_t` must match the on-disk WAD header layout: 12
    /// packed bytes.
    #[test]
    fn wadinfo_size_matches_c() {
        // C wadinfo_t = ident[4] + numlumps + infotableofs
        // On x86_64: 4 + 4 + 4 = 12 bytes
        assert_eq!(std::mem::size_of::<wadinfo_t>(), 12);
    }

    /// `filelump_t` must match the on-disk directory-entry layout:
    /// 16 packed bytes.
    #[test]
    fn filelump_size_matches_c() {
        // C filelump_t = filepos + size + name[8]
        // On x86_64: 4 + 4 + 8 = 16 bytes
        assert_eq!(std::mem::size_of::<filelump_t>(), 16);
    }

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
        /// Original `lumpinfo` pointer to restore on drop.
        saved_lumpinfo: *mut lumpinfo_t,
        /// Original `numlumps` count to restore on drop.
        saved_numlumps: c_uint,
        /// Original `lumphash` pointer to restore on drop.
        saved_lumphash: *mut *mut lumpinfo_t,
    }

    impl WadTestScope {
        /// Save the current globals and install `info` /`count` /
        /// `null` for the duration of the test. Returns the guard.
        ///
        /// # Safety
        ///
        /// `info` must remain live for the lifetime of the returned
        /// guard. The caller must serialise with `WAD_LOCK`.
        unsafe fn install(info: *mut lumpinfo_t, count: c_uint) -> Self {
            let scope = WadTestScope {
                saved_lumpinfo: lumpinfo,
                saved_numlumps: numlumps,
                saved_lumphash: lumphash,
            };
            lumpinfo = info;
            numlumps = count;
            lumphash = ptr::null_mut(); // force linear scan, not hash table
            scope
        }
    }

    /// RAII restoration of the saved WAD globals on test teardown.
    impl Drop for WadTestScope {
        /// Restore the saved globals.
        fn drop(&mut self) {
            unsafe {
                lumpinfo = self.saved_lumpinfo;
                numlumps = self.saved_numlumps;
                lumphash = self.saved_lumphash;
            }
        }
    }

    /// Build a `[c_char; 8]` lump name from a byte slice, NUL-padding
    /// or truncating to 8 bytes.
    fn make_lump_name(s: &[u8]) -> [c_char; 8] {
        let mut name = [0i8; 8];
        for (i, &b) in s.iter().take(8).enumerate() {
            name[i] = b as c_char;
        }
        name
    }

    /// Build a fake `wad_file_t` whose `mapped` pointer is non-null,
    /// so `W_ReleaseLumpNum` takes the memory-mapped no-op branch
    /// and avoids the zone allocator (which is not initialised in
    /// the test harness).
    fn mapped_wad() -> wad_file_t {
        static SENTINEL: u8 = 0;
        wad_file_t {
            file_class: ptr::null_mut(),
            mapped: std::ptr::addr_of!(SENTINEL).cast_mut(),
            length: 0,
        }
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

        unsafe { W_ReleaseLumpName(c"TESTLUMP".as_ptr()) };
    }

    /// The underlying `strncasecmp` lookup is case-insensitive, so
    /// "testlump" must resolve to the same entry as "TESTLUMP".
    #[test]
    fn release_lump_name_is_case_insensitive() {
        let _lock = WAD_LOCK.lock().unwrap();
        let mut wad = mapped_wad();
        let mut lumps = [make_lump(b"TESTLUMP", &mut wad)];
        let _scope = unsafe { WadTestScope::install(lumps.as_mut_ptr(), 1) };

        unsafe { W_ReleaseLumpName(c"testlump".as_ptr()) };
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

        unsafe { W_ReleaseLumpName(c"ALPHA".as_ptr()) };
        unsafe { W_ReleaseLumpName(c"BETA".as_ptr()) };
    }
}
