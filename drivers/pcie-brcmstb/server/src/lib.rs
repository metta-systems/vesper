#![no_std]

//! The `pci_v1` interface, generated from
//! `drivers/pcie-brcmstb/interfaces/pci_v1.if`: its types, the
//! [`pci_v1::PciV1`] trait the PCIe host bridge driver implements, and
//! `pci_v1_export!`, which exports an implementation's functions as PPC
//! entries; and [`claimed`], the record `claim` leaves in the data area.

include!(concat!(env!("OUT_DIR"), "/pci_v1.rs"));

/// The record `claim` writes into the reference it is given, as 64-bit words
/// (`DataArea::write_u64s`), offsets in bytes: a header, then one entry per
/// memory BAR it installed Frames for.
pub mod claimed {
    /// Word 0: vendor ID in bits 15:0, device ID in bits 31:16.
    pub const IDENTITY: u64 = 0;
    /// Word 1: base class, subclass and programming interface.
    pub const CLASS: u64 = 8;
    /// Word 2: the bits an MSI of the function signals on the client's
    /// Notification; zero when it has none.
    pub const MSI_BITS: u64 = 16;
    /// Word 3: how many BAR entries follow.
    pub const BAR_COUNT: u64 = 24;
    /// Offset of the first BAR entry.
    pub const BARS: u64 = 32;
    /// Words of a BAR entry: the BAR's number, its bus address, its CPU
    /// address and its size in bytes.
    pub const BAR_WORDS: usize = 4;
    /// Most BAR entries.
    pub const MAX_BARS: usize = 6;
    /// Bytes of the whole record.
    pub const BYTES: u64 = BARS + (MAX_BARS * BAR_WORDS * 8) as u64;
}
