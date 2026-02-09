use {
    libaddress::{PhysAddr, VirtAddr},
    libmapping::AttributeFields,
};

/// What kinds of entries a given table level supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelCapabilities {
    /// This level can contain pointers to the next-level table.
    pub supports_table_pointer: bool,
    /// This level can contain block/page mappings to physical memory.
    pub supports_block: bool,
    /// If block mappings are supported, the block size in bytes.
    /// For the leaf level (e.g. L3 with 4K pages), this is the page size.
    pub block_size: usize,
}

/// A decoded translation table entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Entry is invalid / not present.
    Invalid,
    /// Entry points to the next-level translation table at the given physical address.
    Table(PhysAddr),
    /// Entry maps a block (or page) of physical memory.
    Block(PhysAddr),
}

/// Architecture-specific translation table format.
///
/// Implementations of this trait encode all the knowledge about a specific
/// MMU translation scheme: how many levels, how entries are encoded, what
/// block sizes each level supports, etc.
///
/// The trait uses only associated functions (no `&self`) so implementations
/// are zero-sized type tags (e.g. `struct Aarch64_4K;`).
///
/// ## Hierarchical vs Hashed Page Tables
///
/// Most architectures (AArch64, x86_64, RISC-V) use hierarchical multi-level
/// page tables where each entry is a single u64. For these, the default
/// `ENTRY_WIDTH` of 1 applies and the standard encode/decode methods work
/// directly.
///
/// Some architectures (PowerPC 970MP) use hashed page tables (HPT) with
/// wider entries (e.g. 16 bytes = 2 × u64). These set `ENTRY_WIDTH` to 2
/// and override the `_wide` encode/decode methods that operate on `&[u64]`
/// slices. The single-u64 methods have default panicking implementations
/// for HPT architectures since they are not meaningful.
pub trait TranslationArch {
    /// Number of levels in the translation hierarchy (e.g. 4 for AArch64 4K granule).
    /// For hashed page tables, this is 1 (the hash table itself).
    const NUM_LEVELS: usize;

    /// Number of u64 words per entry. Default is 1 for hierarchical page tables.
    /// PowerPC HPT uses 2 (16-byte PTEs: pte_hi + pte_lo).
    const ENTRY_WIDTH: usize = 1;

    /// Whether this architecture uses a hashed (non-hierarchical) page table.
    /// When true, `index_from_vaddr` may not be meaningful — use
    /// `hash_from_vaddr` instead.
    const HASHED: bool = false;

    /// Number of entries in a table at the given level.
    fn entries_per_table(level: usize) -> usize;

    /// Required alignment in bytes for a table at the given level.
    fn table_alignment(level: usize) -> usize;

    /// Size in bytes of a table at the given level.
    /// For ENTRY_WIDTH=1: entries * 8. For ENTRY_WIDTH=2: entries * 16.
    fn table_size(level: usize) -> usize {
        Self::entries_per_table(level) * Self::ENTRY_WIDTH * core::mem::size_of::<u64>()
    }

    /// What kinds of entries this level supports.
    fn level_capabilities(level: usize) -> LevelCapabilities;

    /// Extract the table index from a virtual address for a given level.
    /// For hierarchical page tables, this extracts the VPN bits.
    /// For hashed page tables, use `hash_from_vaddr` instead.
    fn index_from_vaddr(vaddr: VirtAddr, level: usize) -> usize;

    // -- Single-u64 entry methods (hierarchical page tables) --

    /// Decode a raw 64-bit entry at a given level into its semantic meaning.
    /// Default implementation panics for hashed architectures.
    fn decode_entry(raw: u64, level: usize) -> EntryKind;

    /// Encode a table pointer entry (pointing to the next-level table).
    fn encode_table_entry(next_table_phys: PhysAddr, level: usize) -> u64;

    /// Encode a block mapping entry at levels that support blocks (L1 1G, L2 2M).
    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64;

    /// Encode a page mapping entry at the leaf level (L3 for 4K granule).
    fn encode_page_entry(phys: PhysAddr, attr: AttributeFields) -> u64;

    /// Extract the output physical address from a raw entry at a given level.
    fn output_address(raw: u64, level: usize) -> PhysAddr;

    // -- Wide entry methods (hashed page tables with ENTRY_WIDTH > 1) --

    /// Decode a wide entry (multiple u64 words) at a given level.
    /// The slice length must equal `ENTRY_WIDTH`.
    /// Default implementation delegates to `decode_entry` for ENTRY_WIDTH=1.
    fn decode_entry_wide(raw: &[u64], level: usize) -> EntryKind {
        debug_assert_eq!(raw.len(), Self::ENTRY_WIDTH);
        Self::decode_entry(raw[0], level)
    }

    /// Encode a page mapping into a wide entry.
    /// Returns the entry words. Default delegates to `encode_page_entry`.
    fn encode_page_entry_wide(phys: PhysAddr, attr: AttributeFields, buf: &mut [u64]) {
        debug_assert_eq!(buf.len(), Self::ENTRY_WIDTH);
        buf[0] = Self::encode_page_entry(phys, attr);
    }

    /// Extract the output physical address from a wide entry.
    /// Default delegates to `output_address`.
    fn output_address_wide(raw: &[u64], level: usize) -> PhysAddr {
        debug_assert_eq!(raw.len(), Self::ENTRY_WIDTH);
        Self::output_address(raw[0], level)
    }

    // -- Hash-based lookup (hashed page tables) --

    /// For hashed page tables: compute the primary hash index (PTEG index)
    /// from a virtual address and the VSID (Virtual Segment ID).
    ///
    /// Returns the PTEG index within the hash table.
    /// Default implementation returns 0 (unused for hierarchical tables).
    fn hash_primary(_vaddr: VirtAddr, _vsid: u64, _htab_mask: u64) -> usize {
        0
    }

    /// For hashed page tables: compute the secondary hash index.
    /// Default implementation returns 0 (unused for hierarchical tables).
    fn hash_secondary(_primary_hash: usize, _htab_mask: u64) -> usize {
        0
    }
}
