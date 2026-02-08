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
pub trait TranslationArch {
    /// Number of levels in the translation hierarchy (e.g. 4 for AArch64 4K granule).
    const NUM_LEVELS: usize;

    /// Number of entries in a table at the given level.
    fn entries_per_table(level: usize) -> usize;

    /// Required alignment in bytes for a table at the given level.
    fn table_alignment(level: usize) -> usize;

    /// Size in bytes of a table at the given level (entries * 8).
    fn table_size(level: usize) -> usize {
        Self::entries_per_table(level) * core::mem::size_of::<u64>()
    }

    /// What kinds of entries this level supports.
    fn level_capabilities(level: usize) -> LevelCapabilities;

    /// Extract the table index from a virtual address for a given level.
    fn index_from_vaddr(vaddr: VirtAddr, level: usize) -> usize;

    /// Decode a raw 64-bit entry at a given level into its semantic meaning.
    fn decode_entry(raw: u64, level: usize) -> EntryKind;

    /// Encode a table pointer entry (pointing to the next-level table).
    fn encode_table_entry(next_table_phys: PhysAddr, level: usize) -> u64;

    /// Encode a block mapping entry at levels that support blocks (L1 1G, L2 2M).
    fn encode_block_entry(phys: PhysAddr, attr: AttributeFields, level: usize) -> u64;

    /// Encode a page mapping entry at the leaf level (L3 for 4K granule).
    fn encode_page_entry(phys: PhysAddr, attr: AttributeFields) -> u64;

    /// Extract the output physical address from a raw entry at a given level.
    fn output_address(raw: u64, level: usize) -> PhysAddr;
}
