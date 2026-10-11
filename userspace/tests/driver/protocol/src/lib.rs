#![no_std]

//! What driver-test and its probe driver agree on.
//!
//! Kickstart loads one `driver-probe` instance per enabled node that names
//! one of its compatible strings. Before each instance starts, driver-test
//! installs a report Notification key at [`REPORT_SLOT`] of its table, badged
//! with a bit for that instance; the probe signals it once every check
//! passed.

use libobject::{KeySlot, RawKey};

/// Slot of the probe's table holding the report key.
pub const REPORT_SLOT: u32 = 250;

/// The probe's compatible strings, by index (`DriverInit::compatible_index`).
pub const PL011: u64 = 0;
pub const AUX: u64 = 1;
pub const AUX_UART: u64 = 2;

/// Width of a component table's slot index (its tables are 256 entries).
const SLOT_BITS: u32 = 8;

/// The report key, in the table the probe's own table key `table` names.
pub fn report_key(table: u64) -> RawKey {
    let guard_bits = RawKey::from_wire(table).slot().0 & !((1 << SLOT_BITS) - 1);
    RawKey::new(KeySlot(guard_bits | REPORT_SLOT), 1)
}
