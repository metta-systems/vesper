//! The component's heap: one 2 MiB Frame carved from its RAM Untyped, and the
//! global allocator over it (`libheap`).
//!
//! What lives there: the bank's zones (about 70 KiB for the trimmed SC-55),
//! the song being collected and the parsed song's events (a few hundred KiB
//! for the largest shareware track). The sample data stays in the resource's
//! mapping.

use {libdevice_driver::DriverInit, libheap::Heap, libobject::CapError};

/// Where the heap is mapped.
const HEAP_VA: u64 = 0x1000_0000;

/// The component's Thread allocates at start, `service` on the caller's
/// Thread afterwards; the heap's lock keeps both sound.
#[global_allocator]
static HEAP: Heap = Heap::new();

/// Carve the heap Frame from the component's RAM Untyped into slot
/// `slot` of its table, map it and give it to the allocator.
pub fn carve(init: &DriverInit, slot: u32) -> Result<(), CapError> {
    HEAP.carve(init, slot, 1, HEAP_VA).map(|_next_slot| ())
}
