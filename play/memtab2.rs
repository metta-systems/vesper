//# snafu = "*"
//
// Explore memory table abstractions
//
// #![feature(decl_macro)]
use core::marker::PhantomData;

/// Provides means to extract the next table level index or the block address.
trait TableIndex {
    // Mask and shift to extract entry index from virt address
    const MASK_BITS: usize;
    const MASK: u64 = 1 << Self::MASK_BITS - 1;
    const SHIFT: usize;
    fn extract_from(address: VirtAddr) -> usize {
        return ((address >> Self::SHIFT) & Self::MASK)
            .try_into()
            .expect("Arithmetics gone mad");
    }
}

enum TableError {
    NotPresent,
}

trait TableOnly {
    type NextTable;
    fn next_table<T: TableIndex>(&self, virt: VirtAddr) -> Result<Self::NextTable, TableError>;
}

enum BlockError {
    NotPresent,
}

trait BlockOnly {
    type Block;
    fn block<T: TableIndex>(&self, virt: VirtAddr) -> Result<Self::Block, BlockError>;
}

trait TableOrBlock: TableOnly + BlockOnly {}

// Higher-level wrapper struct for Stages
struct PageTable<Stage, TableIndex> {
    _marker1: PhantomData<Stage>,
    _marker2: PhantomData<TableIndex>,
}

impl<Stage, TI: TableIndex> PageTable<Stage, TI> where Stage: TableOnly {
    fn next_table(virt: VirtAddr) -> Result<Stage::NextTable, TableError> {
        Stage::next_table<TI>(virt)
    }
}

impl<Stage, TI: TableIndex> PageTable<Stage, TI> where Stage: BlockOnly {
    fn block(virt: VirtAddr) -> Result<Stage::Block, BlockError> {
        Stage::block<TI>(virt)
    }
}

// Specific table structures:
struct Stage0_Gran4k {}
struct Stage1_Gran4k {}
struct Stage2_Gran4k {}
struct Stage3_Gran4k {}

macro_rules! impl_table_only {
    { $stage:ty, $next_stage:ty, $table_index:ty } => {
        impl TableOnly for $stage {
            type NextTable = PageTable::<$next_stage,TableIndex=??>;
            fn next_table<$table_index>(&self, virt: VirtAddr) -> Result<Self::NextTable> {
                let index = $table_index::extract_from(virt);
                let entry = self.table[index];
                if !bit_set(entry, P) {
                    return Err(NotPresent);
                }
                let base = ??::extract_base(entry); // This involves some stage-dependent shenanigans, e.g. for RISC-V the PN[x] entries must be combined depending on the stage.
                let next_table = base as *mut Self::NextTable .. ?;
                Ok(next_table)
            }
        }
    }
}

macro_rules! impl_block_only {
    { $stage:ty, $block:ty, $table_index:ty } => {
        impl BlockOnly for Stage3 {
            type Block = Frame<Size4K>; // we will get a block of given size at this stage
            fn<TI: TableIndex> block<TI>(virt: VirtAddr) -> Result<Self::Block> {
                TI::extract(virt)
                // convert to Result<Block>
            }
        }
    }
}

impl_table_only!(Stage0, Stage1, TableIndex_Stage0);
impl_table_only!(Stage1, Stage2, TableIndex_Stage1);
impl_block_only!(Stage1, Size1G, TableIndex_Stage1);
impl_table_only!(Stage2, Stage3, TableIndex_Stage2);
impl_block_only!(Stage2, Size2M, TableIndex_Stage2);
impl_block_only!(Stage3, Size4K, TableIndex_Stage3);

// now for the TableOnly+BlockOnly combination we need to also provide a resolution method that returns either
impl_next_level!(Stage1, Stage2, Size1G, TableIndex_Stage1); // this provides next() -> Either<Table,Block>
impl_next_level!(Stage2, Stage3, Size2M, TableIndex_Stage2);

// @todo need to also provide next() for TableOnly and for BlockOnly types separately, without Either?

// Level could be determining which of TableOnly, BlockOnly are implemented? (or implement Table/Block only FOR the Level?)
// Mask (TableIndex) gives extraction parameters for next table or next block.

// Granule is not essential - it's arch specific and translates into number of Stages and
// TableIndex for ptr extraction.
