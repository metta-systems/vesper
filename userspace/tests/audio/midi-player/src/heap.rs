//! The player's heap: one 2 MiB Frame carved from its RAM Untyped, and the
//! global allocator over it (`libheap`).
//!
//! What lives there: the parsed song's events and tempo map, read once at
//! start for the log; the song's bytes stay in the resource's mapping.

use {libdevice_driver::DriverInit, libheap::Heap, libobject::CapError};

/// Where the heap is mapped.
const HEAP_VA: u64 = 0x1000_0000;

#[global_allocator]
static HEAP: Heap = Heap::new();

/// Carve the heap Frame from the player's RAM Untyped into slot `slot` of
/// its table, map it and give it to the allocator.
pub fn carve(init: &DriverInit, slot: u32) -> Result<(), CapError> {
    HEAP.carve(init, slot, 1, HEAP_VA).map(|_next_slot| ())
}
