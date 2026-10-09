//! `AArch64` page-table kernel metadata and descriptor mechanics.
//!
//! A `PageTable` capability names a Retype-carved 4 KiB physical table plus
//! this kernel metadata: the carve address, the walk level (0 = translation
//! root), and the installation record (parent table and slot, or the owning
//! `AddressSpace` for a root). The hardware-format table itself is ordinary RAM
//! in the Untyped's committed range, reached through the direct map.
//!
//! Descriptor format (`AArch64` Stage 1, 4 KiB granule, 48-bit VA): matches the
//! boot-time configuration set by Kickstart — MAIR index 0 is normal
//! write-back cacheable memory, and every level uses 9-bit indices.

use {
    crate::objects::{
        NucleusObject,
        access::ObjectId,
        arch_objects::{PageTableObject, PtParent},
    },
    libaddress::PhysAddr,
    libobject::{CapError, ObjectType},
};

/// Descriptor flag bits for `AArch64` Stage 1.
pub mod pte {
    /// Valid bit.
    pub const VALID: u64 = 1 << 0;
    /// Table descriptor (levels 0–2) and page descriptor (level 3), bit 1.
    /// Block descriptors (levels 1–2) have this bit clear.
    pub const DESCRIPTOR: u64 = 1 << 1;
    /// Access flag: hardware does not fault on first access.
    pub const AF: u64 = 1 << 10;
    /// Non-global: the translation is tagged with this `AddressSpace`'s ASID.
    pub const NG: u64 = 1 << 11;
    /// Inner shareable.
    pub const SH_INNER: u64 = 0b11 << 8;
    /// AP[2:1] = 0b00: readable and writable at EL1 only (EL0 denied).
    /// Required for privileged executable mappings: EL1 cannot execute
    /// EL0-writable pages (the architectural user-writable execute-never
    /// rule), so a kernel-privilege RW+X mapping must deny EL0.
    pub const AP_RW_EL1: u64 = 0b00 << 6;
    /// AP[2:1] = 0b01: readable and writable at EL0 and EL1.
    pub const AP_RW_USER: u64 = 0b01 << 6;
    /// AP[2:1] = 0b11: read-only at EL0 and EL1.
    pub const AP_RO_USER: u64 = 0b11 << 6;
    /// Memory attribute index 0 (normal write-back cacheable).
    pub const ATTR_NORMAL: u64 = 0 << 2;
    /// Privileged execute never.
    pub const PXN: u64 = 1 << 53;
    /// Unprivileged execute never.
    pub const UXN: u64 = 1 << 54;
}

/// Entries per table level: 9-bit indices in a 4 KiB table.
pub const ENTRIES_PER_TABLE: usize = 512;

/// Supported virtual address width: a 4-level walk at 4 KiB granule.
pub const VA_BITS: u32 = 48;

/// Output-address bits [47:12] of a page/block/table descriptor.
const ENTRY_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

/// The hardware-format table: 512 entries × 8 bytes = 4 KiB.
#[repr(C, align(4096))]
pub struct RawPageTable {
    pub entries: [u64; ENTRIES_PER_TABLE],
}

/// Kernel metadata for one Retype-carved page table.
pub struct AArch64PageTable {
    /// Physical address of the carved 4 KiB table.
    pub paddr: u64,
    /// Walk level: 0 is the translation root, 3 the leaf-level table.
    pub level: u8,
    /// Installation record.
    pub parent: PtParent,
}

impl AArch64PageTable {
    /// A freshly carved table: not yet installed anywhere.
    pub const fn new(paddr: u64) -> Self {
        Self {
            paddr,
            level: 0,
            parent: PtParent::Uninstalled,
        }
    }
}

impl PageTableObject for AArch64PageTable {
    fn paddr(&self) -> u64 {
        self.paddr
    }

    fn level(&self) -> u8 {
        self.level
    }

    fn is_installed(&self) -> bool {
        !matches!(self.parent, PtParent::Uninstalled)
    }

    fn parent(&self) -> PtParent {
        self.parent
    }

    fn install_root(&mut self, address_space: ObjectId) {
        self.level = 0;
        self.parent = PtParent::Root { address_space };
    }

    fn install_table(&mut self, parent_paddr: u64, parent_level: u8, slot: u16) {
        self.level = parent_level + 1;
        self.parent = PtParent::Table { parent_paddr, slot };
    }

    fn uninstall(&mut self) {
        self.parent = PtParent::Uninstalled;
    }
}

impl NucleusObject for AArch64PageTable {
    const TYPE: ObjectType = ObjectType::PAGE_TABLE;
    const POOL: crate::objects::access::PoolTag = crate::objects::access::PoolTag::PageTable;
}

// ═══════════════════════════════════════════════════════════════════
// DESCRIPTOR MECHANICS
// ═══════════════════════════════════════════════════════════════════

/// Index of `vaddr` in the table at walk `level` (0 = root).
///
/// Level 0 selects bits [47:39], level 1 [38:30], level 2 [29:21], and
/// level 3 [20:12].
#[inline]
pub fn slot_index(level: u8, vaddr: u64) -> usize {
    let shift = 39 - 9 * u32::from(level);
    ((vaddr >> shift) & 0x1FF) as usize
}

/// The leaf level a frame of `size_bits` occupies: 4 KiB pages live in level 3
/// tables, 2 MiB blocks are descriptors of level 2 tables, and 1 GiB blocks of
/// level 1 tables.
#[inline]
pub fn leaf_level(size_bits: u8) -> u8 {
    match size_bits {
        21 => 2,
        30 => 1,
        // 4 KiB pages and any other value land at level 3: sizes are
        // architecture-validated upstream (`validate_frame_size`), so the
        // fallback only keeps the shift arithmetic total.
        _ => 3,
    }
}

/// Access the raw table at physical address `paddr` through the direct map.
///
/// # Safety
/// `paddr` must name a live carved 4 KiB table page (Retype-carved or
/// boot-carved, never freed under the accepted-leak model).
#[expect(
    clippy::missing_safety_doc,
    reason = "the caller-facing contract is on the public wrappers below"
)]
unsafe fn raw_table(paddr: u64) -> &'static mut RawPageTable {
    // SAFETY: the caller guarantees a live carved table at `paddr`.
    unsafe {
        &mut *PhysAddr::new(paddr)
            .user_to_kernel()
            .as_mut_ptr::<RawPageTable>()
    }
}

/// Install a table descriptor for `child_paddr` in the parent table at
/// `parent_paddr` (walk level `parent_level`), at the slot selected by
/// `vaddr`.
///
/// The selected slot must be vacant; an occupied slot is a mapping conflict.
/// Returns the installed slot index, part of the installation record.
pub fn install_table_entry(
    parent_paddr: u64,
    parent_level: u8,
    vaddr: u64,
    child_paddr: u64,
) -> Result<u16, CapError> {
    // SAFETY: the caller (the PageTable.Map handler) resolved the parent
    // through a validated capability naming a live carved table.
    let parent = unsafe { raw_table(parent_paddr) };
    let slot = slot_index(parent_level, vaddr);
    if parent.entries[slot] != 0 {
        return Err(CapError::AlreadyMapped);
    }
    parent.entries[slot] = child_paddr | pte::VALID | pte::DESCRIPTOR;
    publish_descriptor_store();
    Ok(u16::try_from(slot).expect("9-bit slot index"))
}

/// Clear the table descriptor at `slot` in the parent table, verifying it
/// still points at `child_paddr` first.
pub fn clear_table_entry(parent_paddr: u64, slot: u16, child_paddr: u64) -> Result<(), CapError> {
    // SAFETY: see `install_table_entry`.
    let parent = unsafe { raw_table(parent_paddr) };
    let entry = parent.entries[usize::from(slot)];
    if entry & pte::VALID == 0 || entry & ENTRY_ADDR_MASK != child_paddr {
        return Err(CapError::InvalidOperation);
    }
    parent.entries[usize::from(slot)] = 0;
    Ok(())
}

/// Whether every descriptor in the table is zero.
pub fn table_is_empty(paddr: u64) -> bool {
    // SAFETY: see `install_table_entry`.
    let table = unsafe { raw_table(paddr) };
    table.entries.iter().all(|&entry| entry == 0)
}

/// Walk from the root at `root_paddr` to the leaf level for `size_bits`,
/// following table descriptors selected by `vaddr`, and return the physical
/// address of the leaf-level table.
fn walk_to_leaf(root_paddr: u64, vaddr: u64, size_bits: u8) -> Result<u64, CapError> {
    let leaf = leaf_level(size_bits);
    let mut table = root_paddr;
    for level in 0..leaf {
        // SAFETY: each step follows a valid table descriptor installed by
        // `install_table_entry`, so the address names a live carved table.
        let entry = unsafe { raw_table(table) }.entries[slot_index(level, vaddr)];
        if entry & pte::VALID == 0 {
            return Err(CapError::MissingIntermediate { vaddr });
        }
        if entry & pte::DESCRIPTOR == 0 {
            // A block already covers this range: the walk cannot proceed to a
            // smaller granularity, and the address is already mapped.
            return Err(CapError::AlreadyMapped);
        }
        table = entry & ENTRY_ADDR_MASK;
    }
    Ok(table)
}

/// Install the page/block descriptor for a frame mapping.
///
/// `vaddr` must be inside the supported virtual-address width and aligned to
/// the frame size; the leaf slot must be vacant. Permissions: `writable`
/// selects read/write versus read-only; `executable` (the `EXECUTE` right)
/// makes the mapping executable at exactly one exception level. A writable
/// executable mapping is kernel-privilege (AP=00, EL0 denied, UXN): EL1
/// cannot execute EL0-writable pages, and EL0 must not execute writable pages
/// either (W^X). A read-only executable mapping is EL0-executable only (AP=11,
/// PXN). Without `EXECUTE` every mapping stays PXN|UXN.
pub fn install_frame_pte(
    root_paddr: u64,
    vaddr: u64,
    frame_paddr: u64,
    size_bits: u8,
    writable: bool,
    executable: bool,
) -> Result<(), CapError> {
    if vaddr >= 1 << VA_BITS {
        return Err(CapError::InvalidPointer);
    }
    let frame_size = 1_u64 << size_bits;
    if vaddr & (frame_size - 1) != 0 {
        return Err(CapError::InvalidSize(size_bits.into()));
    }
    let leaf = leaf_level(size_bits);
    let table = walk_to_leaf(root_paddr, vaddr, size_bits)?;
    // SAFETY: the walk followed installed table descriptors to a live carved
    // leaf-level table.
    let leaf_table = unsafe { raw_table(table) };
    let slot = slot_index(leaf, vaddr);
    if leaf_table.entries[slot] != 0 {
        return Err(CapError::AlreadyMapped);
    }
    leaf_table.entries[slot] = frame_descriptor(frame_paddr, leaf, writable, executable);
    publish_descriptor_store();
    Ok(())
}

/// Publish a new descriptor before it can be consumed by a translation walk.
fn publish_descriptor_store() {
    // SAFETY: these barriers access no memory themselves and clobber no
    // registers. They complete prior descriptor stores and synchronize the
    // execution context before access through a newly installed mapping;
    // ERET alone does not provide the descriptor-store completion.
    unsafe {
        core::arch::asm!("dsb ishst", "isb", options(nostack));
    }
}

/// Encode a TTBR0 leaf without accessing its backing table.
fn frame_descriptor(frame_paddr: u64, leaf: u8, writable: bool, executable: bool) -> u64 {
    let (ap, execute_never) = match (writable, executable) {
        // Kernel-privilege RW+X, executable at EL1 only: EL1 cannot execute
        // EL0-writable pages (the architectural user-writable execute-never
        // rule), so it denies EL0 data access (AP=00), and UXN keeps EL0 from
        // fetching it — AP=00 alone does not stop an EL0 instruction fetch.
        // This is the bootstrap caller's image case.
        (true, true) => (pte::AP_RW_EL1, pte::UXN),
        // Read-only, executable at EL0 only (AP=11, PXN): EL1 never executes
        // code an EL0 component can map.
        (false, true) => (pte::AP_RO_USER, pte::PXN),
        (true, false) => (pte::AP_RW_USER, pte::PXN | pte::UXN),
        (false, false) => (pte::AP_RO_USER, pte::PXN | pte::UXN),
    };
    // Level 3 uses page descriptors (bit 1 set); levels 1–2 use block
    // descriptors (bit 1 clear).
    let descriptor_bit = if leaf == 3 { pte::DESCRIPTOR } else { 0 };
    frame_paddr
        | pte::VALID
        | descriptor_bit
        | pte::AF
        | pte::NG
        | pte::SH_INNER
        | ap
        | pte::ATTR_NORMAL
        | execute_never
}

/// Clear the page/block descriptor of the frame mapping at `vaddr`, verifying it
/// still points at `frame_paddr` first.
pub fn clear_frame_pte(
    root_paddr: u64,
    vaddr: u64,
    frame_paddr: u64,
    size_bits: u8,
) -> Result<(), CapError> {
    let leaf = leaf_level(size_bits);
    let table = walk_to_leaf(root_paddr, vaddr, size_bits)?;
    // SAFETY: see `install_frame_pte`.
    let leaf_table = unsafe { raw_table(table) };
    let slot = slot_index(leaf, vaddr);
    let entry = leaf_table.entries[slot];
    if entry & pte::VALID == 0 || entry & ENTRY_ADDR_MASK != frame_paddr {
        return Err(CapError::InvalidOperation);
    }
    leaf_table.entries[slot] = 0;
    Ok(())
}

/// Alias-policy walk: find a live leaf descriptor whose physical extent
/// overlaps `[start, end)`, returning its physical base.
///
/// Table descriptors (levels 0–2) are followed recursively; block descriptors
/// (levels 1–2) and page descriptors (level 3) are leaves whose extents are
/// checked for interval overlap with the candidate frame extent. The check is
/// physical, not capability-based: any live descriptor covering any byte of
/// the candidate conflicts, whatever capability installed it and whatever
/// frame sizes were involved.
fn overlap_walk(table_paddr: u64, level: u8, start: u64, end: u64) -> Option<u64> {
    // SAFETY: reached from the translation root through installed table
    // descriptors, so the address names a live carved table.
    let table = unsafe { raw_table(table_paddr) };
    for &entry in &table.entries {
        if entry & pte::VALID == 0 {
            continue;
        }
        let output = entry & ENTRY_ADDR_MASK;
        if entry & pte::DESCRIPTOR != 0 && level < 3 {
            if let Some(hit) = overlap_walk(output, level + 1, start, end) {
                return Some(hit);
            }
        } else {
            // Leaf: level 1 blocks cover 1 GiB, level 2 blocks 2 MiB, and
            // level 3 pages 4 KiB.
            let leaf_size = 1_u64 << (12 + 9 * u32::from(3 - level));
            if output < end && start < output + leaf_size {
                return Some(output);
            }
        }
    }
    None
}

/// Find a live descriptor in the walk from `root_paddr` whose physical extent
/// overlaps the frame extent `[paddr, paddr + (1 << size_bits))`, returning
/// the conflicting descriptor's physical base.
///
/// Enforces the selected alias policy — no two virtual addresses for
/// overlapping physical backing within one `AddressSpace` — ahead of the hardware
/// transition, so a rejected mapping leaves every table and record unchanged.
pub fn find_physical_overlap(root_paddr: u64, paddr: u64, size_bits: u8) -> Option<u64> {
    // Frame sizes are architecture-validated upstream
    // (`validate_frame_size`), so the extent cannot wrap the address space.
    let end = paddr + (1_u64 << size_bits);
    overlap_walk(root_paddr, 0, paddr, end)
}

#[cfg(test)]
mod tests {
    use super::{frame_descriptor, leaf_level};

    #[test_case]
    fn ttbr0_pages_and_blocks_are_non_global_for_every_permission_shape() {
        for (size_bits, base, descriptor_bits) in [
            (12, 0x1234_5000, 0b11),
            (21, 0x2460_0000, 0b01),
            (30, 0x4000_0000, 0b01),
        ] {
            for (writable, executable, ap, xn) in [
                (false, false, 0b11 << 6, (1 << 53) | (1 << 54)),
                (true, false, 0b01 << 6, (1 << 53) | (1 << 54)),
                // Read-only code executes at EL0 only (PXN).
                (false, true, 0b11 << 6, 1 << 53),
                // RW+X executes at EL1 only (UXN), EL0 has no access.
                (true, true, 0, 1 << 54),
            ] {
                let descriptor =
                    frame_descriptor(base, leaf_level(size_bits), writable, executable);
                // Literal flag bits pin the hardware format independently of
                // the production constants, including nG at bit 11.
                let expected =
                    base | descriptor_bits | (1 << 10) | (1 << 11) | (0b11 << 8) | ap | xn;
                assert_eq!(descriptor, expected);
            }
        }
    }
}
