#![no_std]

//! What driver-test and its probe driver agree on.
//!
//! Kickstart loads one `driver-probe` instance per enabled node that names
//! one of its compatible strings. Before each instance starts, driver-test
//! installs a report Notification key at [`REPORT_SLOT`] of its table, badged
//! with a bit for that instance; the probe signals it once every check
//! passed.

use {libdevice_driver::local_key, libobject::RawKey};

/// Slot of the probe's table holding the report key.
pub const REPORT_SLOT: u32 = 250;

/// The probe's compatible strings, by index (`DriverInit::compatible_index`).
pub const PL011: u64 = 0;
pub const AUX: u64 = 1;
pub const AUX_UART: u64 = 2;

/// The report key, in the table the probe's own table key `table` names.
pub fn report_key(table: u64) -> RawKey {
    local_key(table, REPORT_SLOT)
}

/// Slot of a client's table holding a key to the table itself with only
/// `INSTALL | LEND`, which mailbox-test installs for the client to lend to
/// the framebuffer component.
pub const LEND_TABLE_SLOT: u32 = 249;

/// Slot of a client's table holding a key to the table itself with only
/// `INSTALL`: a key that cannot be lent.
pub const PLAIN_TABLE_SLOT: u32 = 248;

/// First slot of the client's table the framebuffer component installs
/// Frames into.
pub const FRAME_SLOT: u32 = 100;

/// The lendable table key, in the table the client's own table key `table` names.
pub fn lend_table_key(table: u64) -> RawKey {
    local_key(table, LEND_TABLE_SLOT)
}

/// The table key without `LEND`.
pub fn plain_table_key(table: u64) -> RawKey {
    local_key(table, PLAIN_TABLE_SLOT)
}
