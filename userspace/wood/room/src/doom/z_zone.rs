//! Rust port of vendor/doomgeneric/z_zone.c.
//!
//! Zone Memory Allocation — a doubly-linked-list allocator whose blocks
//! sit inside a buffer returned by `I_ZoneBase`. Callers never inspect
//! the header, so only the user-visible pointer and behaviour matter.
//!
//! Allocations are tagged with a `PU_*` purge level: blocks at or above
//! `PU_PURGELEVEL` can be reclaimed automatically by future `Z_Malloc`
//! calls when space is tight. Tags below `PU_PURGELEVEL` are pinned.

#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types)]

use std::{ffi::c_void, ptr};

use crate::i_error;

/// Pinned allocation; lives for the lifetime of the program unless
/// explicitly freed. Matches `PU_STATIC` in `z_zone.h`.
pub const PU_STATIC: i32 = 1;
/// Tag value marking a block as currently free. Matches `PU_FREE`.
pub const PU_FREE: i32 = 4;
/// Pinned to the current level; mass-freed by `Z_FreeTags(PU_LEVEL, ...)`
/// when the player exits the map. Matches `PU_LEVEL`.
pub const PU_LEVEL: i32 = 5;
/// Pinned to the current level for specials (sector effects, plats,
/// doors). Matches `PU_LEVSPEC`.
pub const PU_LEVSPEC: i32 = 6;
/// First purgeable tag; anything at or above this can be reclaimed by
/// `Z_Malloc`. Matches `PU_PURGELEVEL`.
pub const PU_PURGELEVEL: i32 = 7;
/// Purgeable cache (textures, sprites). Matches `PU_CACHE`.
pub const PU_CACHE: i32 = 8;

/// Magic number stamped into `memblock_t::id` so `Z_Free`/`Z_ChangeTag`
/// can detect double-free and pointer arithmetic mistakes. Mirrors the
/// `ZONEID` macro in `z_zone.c`.
const ZONEID: u32 = 0x1d4a11;
/// Minimum free fragment that justifies splitting a block during
/// `Z_Malloc`. Smaller leftovers stay attached to the allocation as
/// internal padding. Matches `MINFRAGMENT` in `z_zone.c`.
const MINFRAGMENT: i32 = 64;
/// Allocation alignment (pointer size, matching `MEM_ALIGN` in
/// `z_zone.c`). Sizes passed to `Z_Malloc` are rounded up to this.
const MEM_ALIGN: usize = std::mem::size_of::<*mut ()>();

/// Header that precedes every Zone allocation in memory. Matches
/// `memblock_t` in `z_zone.c`; the `id` field carries `ZONEID` so
/// stray frees can be detected.
#[repr(C)]
#[derive(Clone, Copy)]
struct memblock_t {
    /// Total bytes including header and any trailing fragment.
    size: i32,
    /// Optional back-pointer the user gave to `Z_Malloc`; nulled when
    /// the block is freed or purged.
    user: *mut *mut c_void,
    /// `PU_*` tag controlling lifetime / purgeability.
    tag: i32,
    /// `ZONEID` for live blocks, 0 once freed.
    id: i32,
    /// Next block in the doubly-linked free/used list.
    next: *mut memblock_t,
    /// Previous block in the doubly-linked free/used list.
    prev: *mut memblock_t,
}

/// Zone-level header sitting at the start of `mainzone`. Matches the
/// anonymous `memzone_t` struct in `z_zone.c`.
#[repr(C)]
struct memzone_t {
    /// Total bytes of the backing buffer (including this header).
    size: i32,
    /// Sentinel block used as the doubly-linked-list anchor.
    blocklist: memblock_t,
    /// "Next-fit" cursor consulted by `Z_Malloc`.
    rover: *mut memblock_t,
}

/// `memzone_t *mainzone` — the single global Zone instance. Initialised
/// by `Z_Init`; null beforehand. Most allocator entry points dereference
/// this unconditionally, so calls before `Z_Init` are programmer errors.
pub static mut mainzone: *mut memzone_t = ptr::null_mut();

use crate::doom::i_system::I_ZoneBase;

/// `void Z_ClearZone(memzone_t *zone)` — reset `zone` to a single free
/// block spanning the entire backing buffer.
///
/// # Safety
/// - `zone` must point to a writable `memzone_t` whose backing buffer is
///   at least `zone.size` bytes long.
/// - No outstanding pointers into `zone` may exist; this overwrites the
///   entire allocator state.
unsafe fn Z_ClearZone(zone: *mut memzone_t) {
    let block = (zone as *mut u8).add(std::mem::size_of::<memzone_t>()) as *mut memblock_t;

    (*zone).blocklist.next = block;
    (*zone).blocklist.prev = block;
    (*zone).blocklist.user = zone as *mut *mut c_void;
    (*zone).blocklist.tag = 1; // PU_STATIC
    (*zone).rover = block;

    (*block).prev = &mut (*zone).blocklist;
    (*block).next = &mut (*zone).blocklist;
    (*block).tag = PU_FREE;
    (*block).size = (*zone).size - std::mem::size_of::<memzone_t>() as i32;
}

/// `void Z_Init(void)` — request the platform Zone buffer from
/// `I_ZoneBase`, install it as `mainzone`, and clear it to one free
/// block. Called once early in `D_DoomMain`.
///
/// # Safety
/// - Must be called exactly once at program startup, before any other
///   `Z_*` entry point.
/// - Reads/writes the global `mainzone`; not thread-safe.
pub unsafe fn Z_Init() {
    let mut size: i32 = 0;
    mainzone = I_ZoneBase(&mut size) as *mut memzone_t;
    (*mainzone).size = size;

    Z_ClearZone(mainzone);
}

/// `void Z_Free(void *ptr)` — release a block previously returned by
/// `Z_Malloc`. Clears the user back-pointer, marks the block as free,
/// and coalesces with adjacent free neighbours so two consecutive free
/// blocks never coexist.
///
/// # Safety
/// - `ptr` must be a value returned by `Z_Malloc` that has not yet been
///   freed; its header (immediately before `ptr`) must still carry
///   `ZONEID`. Mismatches call `I_Error`.
/// - `mainzone` must be initialised (`Z_Init` ran).
pub unsafe fn Z_Free(ptr: *mut c_void) {
    let block = (ptr as *mut u8).sub(std::mem::size_of::<memblock_t>()) as *mut memblock_t;

    if (*block).id as u32 != ZONEID {
        i_error!("Z_Free: freed a pointer without ZONEID");
    }

    if (*block).tag != PU_FREE && !(*block).user.is_null() {
        *(*block).user = ptr::null_mut();
    }

    (*block).tag = PU_FREE;
    (*block).user = ptr::null_mut();
    (*block).id = 0;

    let mut block = block;
    let other = (*block).prev;
    if (*other).tag == PU_FREE {
        (*other).size += (*block).size;
        (*other).next = (*block).next;
        (*(*block).next).prev = other;

        if block == (*mainzone).rover {
            (*mainzone).rover = other;
        }
        block = other;
    }

    let other = (*block).next;
    if (*other).tag == PU_FREE {
        (*block).size += (*other).size;
        (*block).next = (*other).next;
        (*(*block).next).prev = block;

        if other == (*mainzone).rover {
            (*mainzone).rover = block;
        }
    }
}

/// `void *Z_Malloc(int size, int tag, void *user)` — allocate `size`
/// bytes (rounded up to `MEM_ALIGN`) tagged with `tag`. If `user` is
/// non-null it is treated as a `void **` and stored in the block header
/// so the allocator can clear it on purge / free.
///
/// Walks the next-fit `rover`, evicting purgeable blocks
/// (`tag >= PU_PURGELEVEL`) and merging adjacent free space until a big
/// enough free block is found. Splits oversized blocks only when the
/// leftover would exceed `MINFRAGMENT`.
///
/// The returned buffer is zeroed before being handed back. A non-purgeable
/// allocation without a user back-pointer is permitted; a purgeable one
/// without a user back-pointer is fatal (`I_Error`).
///
/// # Safety
/// - `mainzone` must be initialised (`Z_Init` ran). The function panics
///   if it is null.
/// - If `user` is non-null it must be a valid `*mut *mut c_void` that
///   the caller is willing to have written by both this call and any
///   future purge/free.
/// - `tag` must be a valid `PU_*` value (other values are silently
///   accepted but break the purge logic, matching C).
pub unsafe fn Z_Malloc(size: i32, tag: i32, user: *mut c_void) -> *mut c_void {
    if mainzone.is_null() {
        panic!("Z_Malloc: mainzone is null!");
    }
    let size = (size + MEM_ALIGN as i32 - 1) & !(MEM_ALIGN as i32 - 1);
    let size = size + std::mem::size_of::<memblock_t>() as i32;

    let mut base = (*mainzone).rover;
    if base.is_null() {
        panic!("Z_Malloc: rover is null!");
    }
    if (*base).prev.is_null() {
        panic!(
            "Z_Malloc: rover->prev is null! base={:?} base.tag={} base.size={}",
            base,
            (*base).tag,
            (*base).size
        );
    }
    if (*(*base).prev).tag == PU_FREE {
        base = (*base).prev;
    }

    let mut rover = base;
    let start = (*base).prev;

    loop {
        if rover == start {
            i_error!("Z_Malloc: failed on allocation");
        }

        if (*rover).tag != PU_FREE {
            if (*rover).tag < PU_PURGELEVEL {
                base = rover;
                rover = (*rover).next;
            } else {
                base = (*base).prev;
                let rover_ptr = rover as *mut u8;
                Z_Free(rover_ptr.add(std::mem::size_of::<memblock_t>()) as *mut c_void);
                base = (*base).next;
                rover = (*base).next;
            }
        } else {
            rover = (*rover).next;
        }

        if (*base).tag == PU_FREE && (*base).size >= size {
            break;
        }
    }

    let extra = (*base).size - size;

    if extra > MINFRAGMENT {
        let newblock = (base as *mut u8).add(size as usize) as *mut memblock_t;
        (*newblock).size = extra;
        (*newblock).tag = PU_FREE;
        (*newblock).user = ptr::null_mut();
        (*newblock).prev = base;
        (*newblock).next = (*base).next;
        (*(*base).next).prev = newblock;
        (*base).next = newblock;
        (*base).size = size;
    }

    if user.is_null() && tag >= PU_PURGELEVEL {
        i_error!("Z_Malloc: an owner is required for purgable blocks");
    }

    (*base).user = user as *mut *mut c_void;
    (*base).tag = tag;

    let result = (base as *mut u8).add(std::mem::size_of::<memblock_t>()) as *mut c_void;

    // Zero the user data area using a byte-by-byte loop to avoid
    // memset writing past the end into the next block header.
    // (ptr::write_bytes / memset may use SIMD that overshoots on
    //  non-aligned sizes under ASan instrumentation.)
    let user_size = size - std::mem::size_of::<memblock_t>() as i32;
    let result_bytes = result as *mut u8;
    for i in 0..user_size as usize {
        *result_bytes.add(i) = 0;
    }

    if !(*base).user.is_null() {
        *(*base).user = result;
    }

    (*mainzone).rover = (*base).next;
    (*base).id = ZONEID as i32;

    result
}

/// `void Z_FreeTags(int lowtag, int hightag)` — free every live block
/// whose tag falls in `[lowtag, hightag]`. Called during level shutdown
/// to mass-release `PU_LEVEL` allocations.
///
/// The Rust port adds a 2000-block walk cap and stderr trace
/// (`[Z_FreeTags] ...`) not present in the C original; useful for
/// catching list corruption when porting bugs creep in.
///
/// # Safety
/// - `mainzone` must be initialised. All blocks in the active list must
///   be live `Z_Malloc` allocations (the sentinel's tag is PU_STATIC, so
///   it is skipped by the tag-range check).
pub unsafe fn Z_FreeTags(lowtag: i32, hightag: i32) {
    let zone = mainzone;
    let sentinel = std::ptr::addr_of_mut!((*zone).blocklist);
    let mut block = (*zone).blocklist.next;
    let mut freed = 0;
    let mut walked = 0;

    while block != sentinel && walked < 2000 {
        walked += 1;
        let next = (*block).next;

        if (*block).tag != PU_FREE && (*block).tag >= lowtag && (*block).tag <= hightag {
            let block_ptr = block as *mut u8;
            Z_Free(block_ptr.add(std::mem::size_of::<memblock_t>()) as *mut c_void);
            freed += 1;
        }

        block = next;
    }
    eprintln!(
        "[Z_FreeTags] lowtag={} hightag={} walked={} freed={}",
        lowtag, hightag, walked, freed
    );
}

/// `void Z_DumpHeap(int lowtag, int hightag)` — diagnostic dump of the
/// Zone to stdout. Reports zone size/location, the tag range, and every
/// block whose tag is in `[lowtag, hightag]`, plus inline `ERROR:` lines
/// for invariant violations (size/next mismatch, broken back link,
/// consecutive free blocks).
///
/// # Safety
/// `mainzone` must be initialised.
pub unsafe fn Z_DumpHeap(lowtag: i32, hightag: i32) {
    let zone = mainzone;
    let sentinel = std::ptr::addr_of_mut!((*zone).blocklist);

    println!("zone size: {}  location: {:p}", (*zone).size, zone);
    println!("tag range: {lowtag} to {hightag}");

    let mut block = (*zone).blocklist.next;
    loop {
        if (*block).tag >= lowtag && (*block).tag <= hightag {
            println!(
                "block:{:p}    size:{:7}    user:{:p}    tag:{:3}",
                block,
                (*block).size,
                (*block).user,
                (*block).tag
            );
        }

        if (*block).next == sentinel {
            break;
        }

        if (block as *mut u8).add((*block).size as usize) != (*block).next as *mut u8 {
            println!("ERROR: block size does not touch the next block");
        }

        if (*(*block).next).prev != block {
            println!("ERROR: next block doesn't have proper back link");
        }

        if (*block).tag == PU_FREE && (*(*block).next).tag == PU_FREE {
            println!("ERROR: two consecutive free blocks");
        }

        block = (*block).next;
    }
}

/// `void Z_CheckHeap(void)` — walk the Zone block list and abort via
/// `I_Error` if any invariant is violated (size/next mismatch, broken
/// back link, consecutive free blocks).
///
/// First defers to `Z_CheckHeapQuiet` for a non-fatal pre-pass so that
/// stderr diagnostics show up before the abort.
///
/// # Safety
/// - `mainzone` must be initialised. May call `I_Error` (does not
///   return) on corruption.
pub unsafe fn Z_CheckHeap() {
    if !Z_CheckHeapQuiet() {
        // Already printed diagnostic info.
    }
    let zone = mainzone;
    let sentinel = std::ptr::addr_of_mut!((*zone).blocklist);
    let mut block = (*zone).blocklist.next;

    loop {
        if (*block).next == sentinel {
            break;
        }

        if (block as *mut u8).add((*block).size as usize) != (*block).next as *mut u8 {
            eprintln!(
                "Z_CheckHeap FAIL: block {:?} size={} next={:?} expected_next={:?}",
                block,
                (*block).size,
                (*block).next,
                (block as *mut u8).add((*block).size as usize)
            );
            i_error!("Z_CheckHeap: block size does not touch the next block\n");
        }

        if (*(*block).next).prev != block {
            eprintln!(
                "Z_CheckHeap FAIL: block {:?} next={:?} next.prev={:?}",
                block,
                (*block).next,
                (*(*block).next).prev
            );
            i_error!("Z_CheckHeap: next block doesn't have proper back link\n");
        }

        if (*block).tag == PU_FREE && (*(*block).next).tag == PU_FREE {
            i_error!("Z_CheckHeap: two consecutive free blocks\n");
        }

        block = (*block).next;
    }
}

/// Check heap integrity without aborting. Returns true if valid.
///
/// Rust-only addition (no C counterpart): used by `Z_CheckHeapAfter`
/// and `Z_CheckHeap` for non-fatal diagnostics. Caps the walk at 2000
/// blocks to avoid hanging on a cyclic free-list bug.
///
/// # Safety
/// - Reads `mainzone` and its linked block list; the caller must ensure
///   no other thread mutates the Zone concurrently.
/// - Returns `true` if `mainzone` is null (nothing to check yet).
pub unsafe fn Z_CheckHeapQuiet() -> bool {
    let zone = mainzone;
    if zone.is_null() {
        return true;
    }
    let sentinel = std::ptr::addr_of_mut!((*zone).blocklist);
    let mut block = (*zone).blocklist.next;
    let mut valid = true;
    let mut count = 0;

    while block != sentinel && count < 2000 {
        let size = (*block).size;
        if size <= 0 || size > 10_000_000 {
            eprintln!(
                "Z_CheckHeapQuiet: block {:?} has invalid size {}",
                block, size
            );
            valid = false;
            break;
        }

        // The last block's next points to the sentinel (at the start of the zone),
        // not to block+size (at the end of the zone). This is by design.
        if (*block).next != sentinel {
            let expected_next = (block as *mut u8).add(size as usize) as *mut memblock_t;
            if (*block).next != expected_next {
                eprintln!(
                    "Z_CheckHeapQuiet: block {:?} size={} next={:?} expected={:?}",
                    block,
                    size,
                    (*block).next,
                    expected_next
                );
                valid = false;
            }
        }

        // Check prev link (works for all blocks including the last one whose next is sentinel)
        if !(*block).next.is_null() && (*(*block).next).prev != block {
            eprintln!(
                "Z_CheckHeapQuiet: block {:?} next={:?} next.prev={:?}",
                block,
                (*block).next,
                (*(*block).next).prev
            );
            valid = false;
        }

        // Check for consecutive free blocks (sentinel has tag=PU_STATIC, so skip it)
        if (*block).tag == PU_FREE && (*block).next != sentinel && (*(*block).next).tag == PU_FREE {
            eprintln!(
                "Z_CheckHeapQuiet: two consecutive free blocks at {:?} and {:?}",
                block,
                (*block).next
            );
            valid = false;
        }

        block = (*block).next;
        count += 1;
    }
    if count >= 2000 {
        eprintln!("Z_CheckHeapQuiet: too many blocks, possible loop!");
        valid = false;
    }
    valid
}

/// Rust-only debug helper: run `Z_CheckHeapQuiet` after a logical
/// allocator-touching step and log a labelled error if it fails. No C
/// counterpart; used to localise corruption in tricky porting changes.
///
/// # Safety
/// - Same preconditions as `Z_CheckHeapQuiet`: `mainzone` may be null
///   (nothing to check) but otherwise the list must be quiescent.
pub unsafe fn Z_CheckHeapAfter(name: &str) {
    if !Z_CheckHeapQuiet() {
        eprintln!("*** Heap corruption detected after {} ***", name);
    }
}

/// `void Z_ChangeTag2(void *ptr, int tag, char *file, int line)` —
/// change the tag of an existing block. The `file`/`line` arguments come
/// from the `Z_ChangeTag` macro in `z_zone.h`; the Rust port ignores
/// them (the macro callsite is rewritten on the C side).
///
/// Refuses to mark a block purgeable (`tag >= PU_PURGELEVEL`) without a
/// user back-pointer, since the purge would otherwise leave a dangling
/// reference.
///
/// # Safety
/// - `ptr` must be a live `Z_Malloc` return value (header must carry
///   `ZONEID`); otherwise `I_Error` is called.
/// - `_file` / `_line` are unused but must still be valid pointers per
///   the C calling convention.
pub unsafe fn Z_ChangeTag2(ptr: *mut c_void, tag: i32, _file: *const u8, _line: i32) {
    let block = (ptr as *mut u8).sub(std::mem::size_of::<memblock_t>()) as *mut memblock_t;

    if (*block).id as u32 != ZONEID {
        i_error!("Z_ChangeTag: block without a ZONEID!");
    }

    if tag >= PU_PURGELEVEL && (*block).user.is_null() {
        i_error!("Z_ChangeTag: an owner is required for purgable blocks");
    }

    (*block).tag = tag;
}

/// `void Z_ChangeUser(void *ptr, void **user)` — repoint a block's user
/// back-pointer and store `ptr` into `*user`. Used when the owning
/// structure moves but the data stays put.
///
/// # Safety
/// - `ptr` must be a live `Z_Malloc` return value (header must carry
///   `ZONEID`); otherwise `I_Error` is called.
/// - `user` must be a writable `*mut *mut c_void` whose storage will
///   outlive the block.
pub unsafe fn Z_ChangeUser(ptr: *mut c_void, user: *mut *mut c_void) {
    let block = (ptr as *mut u8).sub(std::mem::size_of::<memblock_t>()) as *mut memblock_t;

    if (*block).id as u32 != ZONEID {
        i_error!("Z_ChangeUser: Tried to change user for invalid block!");
    }

    (*block).user = user;
    *user = ptr;
}

/// `int Z_FreeMemory(void)` — return the number of bytes that could be
/// allocated immediately: the sum of all `PU_FREE` block sizes plus the
/// sizes of purgeable blocks (`tag >= PU_PURGELEVEL`) which can be
/// reclaimed.
///
/// # Safety
/// - `mainzone` must be initialised.
pub unsafe fn Z_FreeMemory() -> i32 {
    let zone = mainzone;
    let sentinel = std::ptr::addr_of_mut!((*zone).blocklist);
    let mut free: i32 = 0;
    let mut block = (*zone).blocklist.next;

    while block != sentinel {
        if (*block).tag == PU_FREE || (*block).tag >= PU_PURGELEVEL {
            free += (*block).size;
        }
        block = (*block).next;
    }

    free
}

/// `void Z_FileDumpHeap(FILE *f)` — stubbed out: the C original wrote a
/// per-block dump to a `FILE *` for the diehard debug build, but no
/// Rust call site exists. Kept as a no-op for API parity.
///
/// # Safety
/// - `_f` is ignored; any value (including null) is accepted.
pub unsafe fn Z_FileDumpHeap(_f: *mut libc::FILE) {
    // No Rust caller found; stubbed per plan.
}

/// `unsigned int Z_ZoneSize(void)` — return the total backing-buffer
/// size in bytes as reported to `Z_Init` by `I_ZoneBase`.
///
/// # Safety
/// - `mainzone` must be initialised; otherwise this dereferences a null
///   pointer.
pub unsafe fn Z_ZoneSize() -> u32 {
    (*mainzone).size as u32
}
