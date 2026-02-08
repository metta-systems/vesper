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
        let raw = *entries.get(index)?;

        match A::decode_entry(raw, level) {
            EntryKind::Invalid => return None,
            EntryKind::Table(next_phys) => {
                table_phys = next_phys;
                // Continue to next level.
            }
            EntryKind::Block(block_phys) => {
                let caps = A::level_capabilities(level);
                let offset = vaddr.as_u64() as usize & (caps.block_size - 1);
                let phys_addr = PhysAddr::new(block_phys.as_u64() + offset as u64);
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
