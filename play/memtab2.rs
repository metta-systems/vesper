//# snafu = "*"
//# paste = "*"
//# either = "*"
//
// Explore memory table abstractions
//
// #![feature(decl_macro)]
use {core::marker::PhantomData, either::*, paste::paste};

/// Provides means to extract the next table level index or the block address.
trait TableIndex {
    // Mask and shift to extract entry index from virt address
    const MASK_BITS: usize;
    const MASK: u64 = 1 << Self::MASK_BITS - 1;
    const SHIFT: usize;
    const BLOCK_ADDR_MASK: u64;
    const TABLE_ADDR_MASK: u64;
    fn extract_index(address: VirtAddr) -> usize {
        return ((address >> Self::SHIFT) & Self::MASK)
            .try_into()
            .expect("Arithmetics gone mad");
    }
    // Extract address of a block with appropriate size (used by BlockOnly)
    fn extract_block_base(entry: u64) {
        entry & Self::BLOCK_ADDR_MASK
    }
    // Extract address of the next table (use by TableOnly) -- aligned to the granule size for VMSAv8
    fn extract_table_base(entry: u64) {
        entry & Self::TABLE_ADDR_MASK
    }
}

// pub enum TraverseError {

#[derive(Snafu, Debug, Clone, Copy, PartialEq)]
enum TableError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    NotPresent,
}

trait TableOnly {
    type NextTable;
    fn next_table(&self, virt: VirtAddr) -> Result<Self::NextTable, TableError>;
    fn extract_table_base(entry: u64) -> PhysAddr; // Extract address of NextTable from this table's entry
}

#[derive(Snafu, Debug, Clone, Copy, PartialEq)]
enum BlockError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    NotPresent,
}

trait BlockOnly {
    type Block;
    fn block(&self, virt: VirtAddr) -> Result<Self::Block, BlockError>;
}

trait TableOrBlock: TableOnly + BlockOnly {}

trait NextStage {
    fn next(&self) -> Either<dyn TableOnly, dyn BlockOnly>;
}

// Higher-level wrapper struct for Stages
struct PageTable<Stage, TableIndex> {
    _marker1: PhantomData<Stage>,
    _marker2: PhantomData<TableIndex>,
    entries: [u64; 1 << TableIndex::MASK_BITS], // u64 must be more flexible type EntryT for other arch's
}

// impl<Stage, TI: TableIndex> PageTable<Stage, TI>
// where
//     Stage: TableOnly,
// {
//     fn next_table(virt: VirtAddr) -> Result<Stage::NextTable, TableError> {
//         Stage::next_table(virt)
//     }
// }

// impl<Stage, TI: TableIndex> PageTable<Stage, TI>
// where
//     Stage: BlockOnly,
// {
//     fn block(virt: VirtAddr) -> Result<Stage::Block, BlockError> {
//         Stage::block(virt)
//     }
// }

// As you can see, the Stage structures are repeated, as the granules (masks), so we need to be able to parameterize them both somehow
// e.g. mask depends on both stage and granule
// These stage tables (or PageTable<Stage> really) can be implemented in terms of TableIndex::MASK_BITS
// e.g. struct Stage<I: TableIndex> { pub entries: [u64; 1 << I::MASK_BITS]; }
// Specific table structures:
struct Stage1_Gran4k {}
struct Stage2_Gran4k {}
struct Stage3_Gran4k {}
struct Stage4_Gran4k {}

struct Stage1_Gran16k {}
struct Stage2_Gran16k {}
struct Stage3_Gran16k {}
struct Stage4_Gran16k {}

struct Stage1_Gran64k {}
struct Stage2_Gran64k {}
struct Stage3_Gran64k {}
struct Stage4_Gran64k {}

macro_rules! impl_table_index {
    { $stage:ty, index = $mask_bits:usize @ $shift:usize, table = $table_bits:usize @ $table_shift:usize, block = $block_bits:usize @ $block_shift:usize } => {
        paste! {
            // struct [< TableIndex_ $stage >];
            impl TableIndex for $stage { //[< TableIndex_ $stage >]
                const MASK_BITS: usize = $mask_bits;
                const SHIFT: usize = $shift;
                const BLOCK_ADDR_MASK: u64 = (1 << $block_bits - 1) << $block_shift;
                const TABLE_ADDR_MASK: u64 = (1 << $table_bits - 1) << $table_shift;
            }
        }
    }
}

impl_table_index!(Stage1_Gran4k, index = 9@39, table = 36@12, block = 0@0);
impl_table_index!(Stage2_Gran4k, index = 9@30, table = 36@12, block = 18@30);
impl_table_index!(Stage3_Gran4k, index = 9@21, table = 36@12, block = 27@21);
impl_table_index!(Stage4_Gran4k, index = 9@12, table = 36@12, block = 36@12);

impl_table_index!(Stage1_Gran16k, index = 1@47);
impl_table_index!(Stage2_Gran16k, index = 11@36);
impl_table_index!(Stage3_Gran16k, index = 11@25);
impl_table_index!(Stage4_Gran16k, index = 11@14);

impl_table_index!(Stage1_Gran64k, index = 5@42);
impl_table_index!(Stage2_Gran64k, index = 13@29);
impl_table_index!(Stage3_Gran64k, index = 13@16);
// impl_table_index!(Stage4_Gran64k, index = 0@0); // N/A

macro_rules! impl_table_only {
    { $stage:ty, $next_stage:ty } => {
        paste! {
            impl TableOnly for $stage {
                type NextTable = PageTable::<$next_stage, [< TableIndex_ $next_stage >] >;
                fn next_table(&self, virt: VirtAddr) -> Result<Self::NextTable> {
                    let index = [< TableIndex_ $stage >]::extract_index(virt);
                    let entry = self.entries[index];
                    if !bit_set(entry, P) {
                        return Err(NotPresent);
                    }
                    let base = $stage::extract_table_base(entry); // This involves some stage-dependent shenanigans, e.g. for RISC-V the PN[x] entries must be combined depending on the stage.
                    let next_table = base as *mut Self::NextTable as &mut Self::NextTable;
                    Ok(next_table)
                }
                fn extract_table_base(entry: u64) -> PhysAddr {
                    [< TableIndex_ $stage >]::extract_base(entry)
                }
            }
        }
    }
}

macro_rules! impl_block_only {
    { $stage:ty, $block_size:ty } => {
        paste! {
            impl BlockOnly for $stage {
                type Block = Frame<$block_size>; // we will get a block of given size at this stage
                fn block<[< TableIndex_ $stage >]>(virt: VirtAddr) -> Result<Self::Block, BlockError> {
                    let index = [< TableIndex_ $stage >]::extract_index(virt)
                    let entry = self.entries[index];
                    if !bit_set(entry, P) {
                        return Err(NotPresent);
                    }
                    let phys_base = $stage::extract_block_base(entry);
                    let block = phys_base as *mut Self::Block as &mut Self::Block;
                    Ok(block)
                }
            }
        }
    }
}

impl_table_only!(Stage0_Gran4k, Stage1_Gran4k);
impl_table_only!(Stage1_Gran4k, Stage2_Gran4k);
impl_block_only!(Stage1_Gran4k, Size1GiB);
impl_table_only!(Stage2_Gran4k, Stage3_Gran4k);
impl_block_only!(Stage2_Gran4k, Size2MiB);
impl_block_only!(Stage3_Gran4k, Size4KiB);

// now for the TableOnly+BlockOnly combination we need to also provide a resolution method that returns either
impl_next_level!(Stage1_Gran4k, Stage2_Gran4k, Size1GiB); // this provides next() -> Either<Table,Block>
impl_next_level!(Stage2_Gran4k, Stage3_Gran4k, Size2MiB);

// @todo need to also provide next() for TableOnly and for BlockOnly types separately, without Either?

// Level could be determining which of TableOnly, BlockOnly are implemented? (or implement Table/Block only FOR the Level?)
// Mask (TableIndex) gives extraction parameters for next table or next block.

// Granule is not essential - it's arch specific and translates into number of Stages and
// TableIndex for ptr extraction.

/// Abstract over the possible page sizes, 4KiB, 16KiB, 2MiB, 1GiB.
pub trait PageSize: Copy + PartialEq + Eq + PartialOrd + Ord {
    /// A string representation of the page size for debug output.
    const SIZE_AS_DEBUG_STR: &'static str;

    /// The page shift in bits.
    const SHIFT: usize;

    /// The page size in bytes.
    const SIZE: usize = 1 << Self::SHIFT;

    /// The page size mask in bits.
    const MASK: u64 = 1 << Self::SHIFT - 1;

    fn alignment() -> usize {
        Self::SIZE
    }

    fn mask() -> u64 {
        Self::MASK
    }
}

//------------------------
// Page: with 4kb granule
//------------------------

/// A standard 4KiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size4KiB {}

impl PageSize for Size4KiB {
    const SIZE_AS_DEBUG_STR: &'static str = "4KiB";
    const SHIFT: usize = 12;
}

//-------------------------
// Page: with 16kb granule
//-------------------------

/// A standard 16KiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size16KiB {}

impl PageSize for Size16KiB {
    const SIZE_AS_DEBUG_STR: &'static str = "16KiB";
    const SHIFT: usize = 14;
}

//-------------------------
// Page: with 64kb granule
//-------------------------

/// A standard 64KiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size64KiB {}

impl PageSize for Size64KiB {
    const SIZE_AS_DEBUG_STR: &'static str = "64KiB";
    const SHIFT: usize = 16;
}

//--------------------------
// Blocks: with 4kb granule
//--------------------------

/// A “huge” 2MiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size2MiB {}

impl PageSize for Size2MiB {
    const SIZE_AS_DEBUG_STR: &'static str = "2MiB";
    const SHIFT: usize = 21;
}

/// A “giant” 1GiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size1GiB {}

impl PageSize for Size1GiB {
    const SIZE_AS_DEBUG_STR: &'static str = "1GiB";
    const SHIFT: usize = 30;
}

//---------------------------
// Blocks: with 16kb granule
//---------------------------

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size32MiB {}

impl PageSize for Size32MiB {
    const SIZE_AS_DEBUG_STR: &'static str = "32MiB";
    const SHIFT: usize = 25;
}

//---------------------------
// Blocks: with 64kb granule
//---------------------------

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size512MiB {}

impl PageSize for Size512MiB {
    const SIZE_AS_DEBUG_STR: &'static str = "512MiB";
    const SHIFT: usize = 29;
}

/// Physical page frame.
struct Frame<P: PageSize> {
    base: usize,
    _page_size: PhantomData<P>,
}

impl<P: PageSize> Frame<P> {
    pub fn new(base: usize) -> Self {
        Self {
            base,
            _page_size: PhantomData,
        }
    }
}
