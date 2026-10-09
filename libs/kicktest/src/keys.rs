//! Key and slot composition for the boot table and fixture-carved tables.
//!
//! A packed key's table-relative address is the table's guard above a
//! `size_bits`-wide slot index. Every table the test kernels carve uses the
//! boot table's size, [`BOOT_TABLE_SIZE_BITS`].

use {
    kickstart::bootstrap::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS},
    libobject::{KeySlot, RawKey},
};

/// The slot half of a key in a table with `guard` (the guard packed above
/// the bare `index`).
pub fn table_slot(guard: u32, index: u32) -> KeySlot {
    KeySlot((guard << u32::from(BOOT_TABLE_SIZE_BITS)) | index)
}

/// A key into a table with `guard`, from a bare slot index and incarnation.
pub fn table_key(guard: u32, index: u32, incarnation: u32) -> RawKey {
    RawKey::from_parts(guard, BOOT_TABLE_SIZE_BITS, index, incarnation)
}

/// A boot-table key from a bare slot index and incarnation.
pub fn boot_key(index: u32, incarnation: u32) -> RawKey {
    table_key(BOOT_TABLE_GUARD, index, incarnation)
}

/// The boot-table slot half for a bare index.
pub fn boot_slot(index: u32) -> KeySlot {
    table_slot(BOOT_TABLE_GUARD, index)
}

/// Hands out consecutive bare boot-table slot indices for a fixture's
/// Retype destinations and bootstrap grants, so a test does not hard-code
/// every slot. Callers pick a starting index clear of the well-known
/// bootstrap slots.
pub struct SlotCursor {
    next: u32,
}

impl SlotCursor {
    pub const fn starting_at(first: u32) -> Self {
        Self { next: first }
    }

    /// Reserve `count` consecutive slots and return the first index.
    pub fn take(&mut self, count: u32) -> u32 {
        let first = self.next;
        self.next = first + count;
        assert!(
            self.next <= 1 << BOOT_TABLE_SIZE_BITS,
            "fixture boot-table slots exhausted"
        );
        first
    }
}
