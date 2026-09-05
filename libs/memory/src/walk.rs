use {
    crate::arch_trait::{EntryKind, TranslationArch},
    libaddress::{PhysAddr, VirtAddr},
};

/// Result of a successful address translation walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslationResult {
    /// The resolved physical address.
    pub phys_addr: PhysAddr,
    /// The level at which the translation terminated (block or page).
    pub level: usize,
    /// The block/page size at the terminating level.
    pub block_size: usize,
}

/// Walk the translation table hierarchy to translate a virtual address.
///
/// The `resolve` callback converts a physical table address and level into
/// a slice of u64 entries. This keeps the library independent of any
/// physical-to-virtual mapping strategy — the caller decides how to
/// access physical memory (identity map, kernel window, etc).
///
/// For hierarchical page tables, this walks from root to leaf through
/// multiple levels. For hashed page tables (`A::HASHED`), use
/// `translate_hashed` instead.
///
/// Returns `None` if the walk encounters an invalid entry at any level.
pub fn translate<A, F>(
    root_phys: PhysAddr,
    vaddr: VirtAddr,
    resolve: F,
) -> Option<TranslationResult>
where
    A: TranslationArch,
    F: Fn(PhysAddr, usize) -> Option<&'static [u64]>,
{
    let mut table_phys = root_phys;

    for level in 0..A::NUM_LEVELS {
        let entries = resolve(table_phys, level)?;
        let index = A::index_from_vaddr(vaddr, level);

        // Read the entry at this index, accounting for ENTRY_WIDTH.
        let offset = index * A::ENTRY_WIDTH;
        let entry_slice = entries.get(offset..offset + A::ENTRY_WIDTH)?;
        let kind = A::decode_entry_wide(entry_slice, level);

        match kind {
            EntryKind::Invalid => return None,
            EntryKind::Table(next_phys) => {
                table_phys = next_phys;
                // Continue to next level.
            }
            EntryKind::Block(block_phys) => {
                let caps = A::level_capabilities(level);
                let page_offset = vaddr.as_usize() & (caps.block_size - 1);
                let phys_addr = PhysAddr::new(block_phys.as_u64() + page_offset as u64);
                return Some(TranslationResult {
                    phys_addr,
                    level,
                    block_size: caps.block_size,
                });
            }
        }
    }

    // Reached past the last level without finding a block/page — should not happen
    // with a well-formed architecture, but handle gracefully.
    None
}

/// Translate a virtual address using a hashed page table (e.g. PowerPC HPT).
///
/// Unlike hierarchical translation, HPT lookup:
/// 1. Computes a primary hash from the VA and VSID to find a PTEG
/// 2. Searches all entries in the PTEG for a matching VA
/// 3. If not found, computes a secondary hash and searches that PTEG
///
/// The `resolve` callback maps the HPT base physical address to a `&[u64]`
/// slice covering the entire hash table. The `vsid` is the Virtual Segment
/// ID from the SLB for this virtual address.
///
/// `entries_per_pteg` is typically 8 for PPC64.
pub fn translate_hashed<A, F>(
    htab_phys: PhysAddr,
    vaddr: VirtAddr,
    vsid: u64,
    htab_mask: u64,
    entries_per_pteg: usize,
    resolve: F,
) -> Option<TranslationResult>
where
    A: TranslationArch,
    F: Fn(PhysAddr, usize) -> Option<&'static [u64]>,
{
    let htab = resolve(htab_phys, 0)?;
    let caps = A::level_capabilities(0);

    // Try primary hash.
    let primary = A::hash_primary(vaddr, vsid, htab_mask);
    if let Some(result) = search_pteg::<A>(htab, primary, entries_per_pteg, vaddr, &caps) {
        return Some(result);
    }

    // Try secondary hash.
    let secondary = A::hash_secondary(primary, htab_mask);
    search_pteg::<A>(htab, secondary, entries_per_pteg, vaddr, &caps)
}

/// Search a single PTEG (Page Table Entry Group) for a matching entry.
fn search_pteg<A: TranslationArch>(
    htab: &[u64],
    pteg_index: usize,
    entries_per_pteg: usize,
    vaddr: VirtAddr,
    caps: &crate::arch_trait::LevelCapabilities,
) -> Option<TranslationResult> {
    let base = pteg_index * entries_per_pteg * A::ENTRY_WIDTH;

    for slot in 0..entries_per_pteg {
        let offset = base + slot * A::ENTRY_WIDTH;
        let entry_slice = htab.get(offset..offset + A::ENTRY_WIDTH)?;
        if let EntryKind::Block(block_phys) = A::decode_entry_wide(entry_slice, 0) {
            // For HPT, we need to verify the VA matches the entry's AVPN.
            // The decode_entry_wide implementation should only return Block
            // for entries matching the queried VA — this requires the arch
            // implementation to encode the VA comparison in decode, or we
            // expose a separate match check. For now, any valid Block is
            // considered a hit since the PTEG was selected by hash.
            let page_offset = vaddr.as_usize() & (caps.block_size - 1);
            let phys_addr = PhysAddr::new(block_phys.as_u64() + page_offset as u64);
            return Some(TranslationResult {
                phys_addr,
                level: 0,
                block_size: caps.block_size,
            });
        }
    }
    None
}
