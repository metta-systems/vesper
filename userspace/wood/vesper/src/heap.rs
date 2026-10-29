//! Wood's memory: the Frames it carves from its RAM Untyped for the engine's
//! stack and heap, and the global allocator over the heap (`libheap`).
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
    libdevice_driver::DriverInit,
    libheap::{FRAME_BYTES, Heap},
    libobject::CapError,
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

/// Wood runs the engine on one Thread, so the heap's lock is never
/// contended; it keeps `alloc` sound if another Thread ever allocates.
#[global_allocator]
static HEAP: Heap = Heap::new();

/// Carve the stack and heap Frames from the program's RAM Untyped into its
/// table from `first_slot`, map them, and give the heap to the allocator.
/// Returns the first slot after the Frames.
pub fn carve(init: &DriverInit, first_slot: u32) -> Result<u32, CapError> {
    let heap_slot = libheap::map_frames(init, first_slot, 1, ENGINE_STACK_VA)?;
    HEAP.carve(init, heap_slot, HEAP_FRAMES, HEAP_VA)
}
