use libmapping::{AccessPermissions, AttributeFields, MemAttributes};

// ---------------------------------------------------------------------------
// AArch64 descriptor bit layout (shared across all granule sizes)
// ---------------------------------------------------------------------------
//
// The descriptor format is identical for 4K, 16K, and 64K granules.
// Only the address masks and index extraction differ per granule.

// Descriptor bit positions
pub(super) const VALID_BIT: u64 = 1 << 0;
pub(super) const TYPE_BIT: u64 = 1 << 1;

// Lower attribute bits
pub(super) const ATTR_INDX_SHIFT: u64 = 2;
pub(super) const AP_SHIFT: u64 = 6;
pub(super) const SH_SHIFT: u64 = 8;
pub(super) const AF_BIT: u64 = 1 << 10;

// Upper attribute bits
pub(super) const PXN_BIT: u64 = 1 << 53;
pub(super) const UXN_BIT: u64 = 1 << 54;

// Table descriptor address mask (bits [47:12], common to all granules)
pub(super) const TABLE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

// AP field values
pub(super) const AP_RW_EL1: u64 = 0b00 << AP_SHIFT;
pub(super) const AP_RO_EL1: u64 = 0b10 << AP_SHIFT;

// SH field values
pub(super) const SH_INNER: u64 = 0b11 << SH_SHIFT;
pub(super) const SH_OUTER: u64 = 0b10 << SH_SHIFT;

/// `MAIR_EL1` attribute indices, matching the MAIR setup in mmu.rs.
pub mod mair {
    pub const NORMAL: u64 = 0;
    pub const NORMAL_NON_CACHEABLE: u64 = 1;
    pub const DEVICE_NGNRE: u64 = 2;
}

/// Encode `AttributeFields` into the lower+upper attribute bits of a
/// block/page descriptor. Shared by all `AArch64` granule implementations.
pub(super) fn encode_attributes(attr: AttributeFields) -> u64 {
    let mut bits: u64 = 0;

    // Memory type -> MAIR index + shareability
    match attr.mem_attributes {
        MemAttributes::CacheableDRAM => {
            bits |= mair::NORMAL << ATTR_INDX_SHIFT;
            bits |= SH_INNER;
        }
        MemAttributes::NonCacheableDRAM => {
            bits |= mair::NORMAL_NON_CACHEABLE << ATTR_INDX_SHIFT;
            bits |= SH_INNER;
        }
        MemAttributes::Device => {
            bits |= mair::DEVICE_NGNRE << ATTR_INDX_SHIFT;
            bits |= SH_OUTER;
        }
    }

    // Access permissions
    bits |= match attr.acc_perms {
        AccessPermissions::ReadWrite => AP_RW_EL1,
        AccessPermissions::ReadOnly => AP_RO_EL1,
    };

    // Execute-never when not executable
    if !attr.executable {
        bits |= PXN_BIT;
    }

    // Always set UXN until userspace is implemented
    bits |= UXN_BIT;

    bits
}
