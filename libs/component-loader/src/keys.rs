//! The boot table's layout and key/slot composition for the boot table and
//! the tables carved for components.
//!
//! A packed key's table-relative address is the table's guard above a
//! `size_bits`-wide slot index. Every table carved for a component uses the
//! boot table's size, [`BOOT_TABLE_SIZE_BITS`].

use libobject::{KeySlot, RawKey};

/// The boot Thread's `KeyTable` capacity exponent: 512 entries, for the
/// well-known bootstrap slots, the boot test's Retype destinations and the
/// slots Kickstart takes while loading every driver and program (it never
/// reuses a deleted one).
pub const BOOT_TABLE_SIZE_BITS: u8 = 9;

/// The boot table's guard:
/// Kickstart picks a nonzero value so every hand-constructed boot key
/// exercises the guard machinery. It fits the 23 guard bits left in a
/// 512-entry table's table-relative address.
pub const BOOT_TABLE_GUARD: u32 = 0x40_FFEE;

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

/// Hands out consecutive bare boot-table slot indices for a builder's
/// Retype destinations and bootstrap grants, so a test does not hard-code
/// every slot. Callers pick a starting index clear of the well-known
/// bootstrap slots.
pub struct SlotCursor {
    next: u32,
    /// A run of slots `take` never hands out, as `(first, end)`
    reserved: (u32, u32),
}

impl SlotCursor {
    pub const fn starting_at(first: u32) -> Self {
        Self {
            next: first,
            reserved: (0, 0),
        }
    }

    /// Never hand out the `count` slots from `first` (slots already holding
    /// boot grants, such as the device Untypeds).
    #[must_use]
    pub const fn avoiding(self, first: u32, count: u32) -> Self {
        Self {
            reserved: (first, first + count),
            ..self
        }
    }

    /// Reserve `count` consecutive slots and return the first index.
    pub fn take(&mut self, count: u32) -> u32 {
        let (reserved_start, reserved_end) = self.reserved;
        if self.next < reserved_end && reserved_start < self.next + count {
            self.next = reserved_end;
        }
        let first = self.next;
        self.next = first + count;
        assert!(
            self.next <= 1 << BOOT_TABLE_SIZE_BITS,
            "boot-table slots exhausted"
        );
        first
    }
}
