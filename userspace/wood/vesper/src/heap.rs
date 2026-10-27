//! Wood's memory: the Frames it carves from its RAM Untyped for the engine's
//! stack and heap, and the global allocator, a TLSF (`rlsf`) over the heap.
//!
//! The engine allocates everything through `alloc`: its zone (6 MiB by
//! default, `-mb` changes it), the 640x400 frame buffer, the WAD directory
//! and the renderer's tables. The WAD itself stays in its resource mapping.
//! [`HEAP_FRAMES`] 2 MiB Frames give it 12 MiB: the default zone, and as
//! much again for everything else. With the stack Frame and the table the
//! window's Frames are lent into, that fits a 16 MiB RAM Untyped.
//!
//! The engine runs on a stack of its own (one 2 MiB Frame at
//! [`ENGINE_STACK_VA`]) rather than the 16 KiB one Kickstart gives a
//! program's Thread: Doom's renderer and level loader keep large arrays on
//! the stack. Nothing is mapped in the 2 MiB below it, so an overflow
//! faults instead of running into other memory.

use {
    core::{
        alloc::{GlobalAlloc, Layout},
        cell::UnsafeCell,
        ptr::{self, NonNull},
        sync::atomic::{AtomicBool, Ordering},
    },
    libcompositor_core::surface::{FRAME_BITS, FRAME_BYTES},
    libdevice_driver::{DriverInit, local_key},
    libobject::{
        CapError, FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
    },
    rlsf::Tlsf,
};

/// Where the engine's stack Frame is mapped; the stack grows down from its
/// end.
pub const ENGINE_STACK_VA: u64 = 0x1000_0000;

/// The engine's stack top.
pub const ENGINE_STACK_TOP: u64 = ENGINE_STACK_VA + FRAME_BYTES;

/// Where the heap's Frames are mapped, one after another, a 2 MiB gap above
/// the stack.
const HEAP_VA: u64 = ENGINE_STACK_TOP + FRAME_BYTES;

/// 2 MiB Frames of heap.
const HEAP_FRAMES: u32 = 6;

/// Bytes of heap.
const HEAP_BYTES: u64 = HEAP_FRAMES as u64 * FRAME_BYTES;

/// Frames carved: the stack's and the heap's.
pub const CARVED_FRAMES: u32 = 1 + HEAP_FRAMES;

/// The TLSF's first-level classes: blocks up to `32 << 20` bytes, more than
/// the whole heap, so the zone fits in one block. Second-level classes
/// split each of them sixteen ways.
const FIRST_LEVELS: usize = 20;
const SECOND_LEVELS: usize = 16;

type Pool = Tlsf<'static, u32, u16, FIRST_LEVELS, SECOND_LEVELS>;

/// The global allocator: a TLSF behind a spin lock. Wood runs the engine on
/// one Thread, so the lock is never contended; it keeps `alloc` sound if
/// another Thread ever allocates.
pub struct Heap {
    locked: AtomicBool,
    pool: UnsafeCell<Pool>,
}

// SAFETY: `pool` is touched only while `locked` is held.
unsafe impl Sync for Heap {}

impl Heap {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            pool: UnsafeCell::new(Tlsf::new()),
        }
    }

    /// Run `body` on the pool, holding the lock.
    fn with_pool<Answer>(&self, body: impl FnOnce(&mut Pool) -> Answer) -> Answer {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: the lock is held, so this is the only reference.
        let answer = body(unsafe { &mut *self.pool.get() });
        self.locked.store(false, Ordering::Release);
        answer
    }
}

#[global_allocator]
static HEAP: Heap = Heap::new();

// SAFETY: the TLSF hands out disjoint blocks of the heap that fit the
// layout, and takes back only what it handed out (the callers' contract).
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.with_pool(|pool| pool.allocate(layout))
            .map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        if let Some(block) = NonNull::new(block) {
            // SAFETY: `block` came from `alloc` with `layout` (the caller's
            // contract).
            self.with_pool(|pool| unsafe { pool.deallocate(block, layout.align()) });
        }
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
            return ptr::null_mut();
        };
        let Some(block) = NonNull::new(block) else {
            return ptr::null_mut();
        };
        // SAFETY: `block` came from `alloc` with `layout`, which has the
        // same alignment as `new_layout` (the caller's contract).
        self.with_pool(|pool| unsafe { pool.reallocate(block, new_layout) })
            .map_or(ptr::null_mut(), NonNull::as_ptr)
    }
}

/// Carve the stack and heap Frames from the program's RAM Untyped into its
/// table from `first_slot`, map them, and give the heap to the allocator.
/// Returns the first slot after the Frames.
pub fn carve(init: &DriverInit, first_slot: u32) -> Result<u32, CapError> {
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    UntypedKey::from_key(RawKey::from_wire(init.memory)).retype(
        ObjectType::FRAME,
        FRAME_BITS,
        0,
        CARVED_FRAMES,
        &own,
        first_slot,
        Rights::all(),
    )?;
    let address_space = RawKey::from_wire(init.address_space);
    let addresses = core::iter::once(ENGINE_STACK_VA)
        .chain((0..u64::from(HEAP_FRAMES)).map(|index| HEAP_VA + index * FRAME_BYTES));
    for (slot, address) in (first_slot..).zip(addresses) {
        FrameKey::from_key(local_key(init.table, slot)).map(
            address_space,
            address,
            Rights(Rights::READ | Rights::WRITE),
            FrameAttribute::Default,
        )?;
    }
    let heap = ptr::slice_from_raw_parts_mut(
        ptr::with_exposed_provenance_mut::<u8>(usize::try_from(HEAP_VA).unwrap_or(0)),
        usize::try_from(HEAP_BYTES).unwrap_or(0),
    );
    if let Some(heap) = NonNull::new(heap) {
        // SAFETY: the heap's Frames are mapped read/write above and nothing
        // else uses them; they stay mapped for the program's lifetime.
        let _inserted = HEAP.with_pool(|pool| unsafe { pool.insert_free_block_ptr(heap) });
    }
    Ok(first_slot + CARVED_FRAMES)
}
