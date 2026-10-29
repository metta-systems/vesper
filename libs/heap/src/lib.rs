#![no_std]

//! A program's heap: 2 MiB Frames carved from its RAM Untyped and mapped one
//! after another, and [`Heap`], a TLSF (`rlsf`) global allocator over them.
//!
//! A program declares the allocator and gives it memory at start:
//!
//! ```ignore
//! #[global_allocator]
//! static HEAP: libheap::Heap = libheap::Heap::new();
//!
//! let next_slot = HEAP.carve(init, first_slot, frames, address)?;
//! ```
//!
//! [`map_frames`] carves and maps Frames without giving them to the
//! allocator, for memory such as a separate stack.

use {
    core::{
        alloc::{GlobalAlloc, Layout},
        cell::UnsafeCell,
        ptr::{self, NonNull},
        sync::atomic::{AtomicBool, Ordering},
    },
    libdevice_driver::{DriverInit, local_key},
    libobject::{
        CapError, FrameAttribute, FrameKey, KeyTableKey, ObjectType, RawKey, Rights, UntypedKey,
    },
    rlsf::Tlsf,
};

/// Size of the Frames carved, as a power of two: 2 MiB.
pub const FRAME_BITS: u8 = 21;

/// Bytes of one Frame.
pub const FRAME_BYTES: u64 = 1 << FRAME_BITS;

/// The TLSF's first-level classes: blocks up to `32 << 20` bytes, more than
/// any program's heap, so one allocation can take most of it. Second-level
/// classes split each of them sixteen ways.
const FIRST_LEVELS: usize = 20;
const SECOND_LEVELS: usize = 16;

type Pool = Tlsf<'static, u32, u16, FIRST_LEVELS, SECOND_LEVELS>;

/// The global allocator: a TLSF behind a spin lock, so allocating stays
/// sound from any of the program's Threads.
pub struct Heap {
    locked: AtomicBool,
    pool: UnsafeCell<Pool>,
}

// SAFETY: `pool` is touched only while `locked` is held.
unsafe impl Sync for Heap {}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    /// An empty heap; [`Heap::carve`] gives it memory.
    pub const fn new() -> Self {
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

    /// Carve `frames` Frames into the program's table from `first_slot`, map
    /// them from `address` on and give them to the allocator. Returns the
    /// first slot after the Frames.
    pub fn carve(
        &self,
        init: &DriverInit,
        first_slot: u32,
        frames: u32,
        address: u64,
    ) -> Result<u32, CapError> {
        let next_slot = map_frames(init, first_slot, frames, address)?;
        let heap = ptr::slice_from_raw_parts_mut(
            ptr::with_exposed_provenance_mut::<u8>(usize::try_from(address).unwrap_or(0)),
            usize::try_from(u64::from(frames) * FRAME_BYTES).unwrap_or(0),
        );
        if let Some(heap) = NonNull::new(heap) {
            // SAFETY: the Frames are mapped read/write above and nothing
            // else uses them; they stay mapped for the program's lifetime.
            let _inserted = self.with_pool(|pool| unsafe { pool.insert_free_block_ptr(heap) });
        }
        Ok(next_slot)
    }
}

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

/// Carve `frames` Frames from the program's RAM Untyped into its table from
/// `first_slot` and map them read/write one after another from `address`.
/// Returns the first slot after the Frames.
pub fn map_frames(
    init: &DriverInit,
    first_slot: u32,
    frames: u32,
    address: u64,
) -> Result<u32, CapError> {
    let own = KeyTableKey::from_key(RawKey::from_wire(init.table));
    UntypedKey::from_key(RawKey::from_wire(init.memory)).retype(
        ObjectType::FRAME,
        FRAME_BITS,
        0,
        frames,
        &own,
        first_slot,
        Rights::all(),
    )?;
    let address_space = RawKey::from_wire(init.address_space);
    for (slot, index) in (first_slot..).zip(0..u64::from(frames)) {
        FrameKey::from_key(local_key(init.table, slot)).map(
            address_space,
            address + index * FRAME_BYTES,
            Rights(Rights::READ | Rights::WRITE),
            FrameAttribute::Default,
        )?;
    }
    Ok(first_slot + frames)
}
