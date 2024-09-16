trait TableIndex {
    // Mask and shift to extract entry index from virt address
    const MASK_BITS: usize;
    const MASK: u64 = 1 << Self::MASK_BITS - 1;
    const SHIFT: usize;
    fn extract(address: VirtAddr) -> usize {
        return ((address >> Self::SHIFT) & Self::MASK)
            .try_into()
            .expect("Arithmetics gone mad");
    }
}

trait TableOnly {
    type NextTable;
    fn<T: TableIndex> next_table<T>(&self, virt: VirtAddr) -> Self::NextTable;
}

trait BlockOnly {
    type Block;
    fn<T: TableIndex> block<T>(&self, virt: VirtAddr) -> Self::Block;
}

trait TableOrBlock: TableOnly + BlockOnly {}


struct PageTable<Stage, TableIndex> {
    _marker1: PhantomData<Stage>,
    _marker2: PhantomData<TableIndex>,
}

impl<Stage, TableIndex> PageTable<Stage, TableIndex> where Stage: TableOnly {
    fn next_table(virt: VirtAddr) -> Result<Stage::NextTable> {
        Stage::next_table<TableIndex>(virt)
    }
}

impl<Stage, TableIndex> PageTable<Stage, TableIndex> where Stage: BlockOnly {
    fn block(virt: VirtAddr) -> Result<Stage::Block> {
        Stage::block<TableIndex>(virt)
    }
}

// TableIndex is external and is controlled by the MMU mode (or granule).
impl TableOnly for Stage0 {
    type NextTable = PageTable<Stage1,TableIndex=??>;
    fn<TI: TableIndex> next_table<TI>(virt: VirtAddr) -> Result<Self::NextTable> {
        TI::extract(virt)
        // convert to Result<NextTable>
    }
}

impl BlockOnly for Stage3 {
    type Block = Frame<Size4K>; // we will get a block of given size at this stage
    fn<TI: TableIndex> block<TI>(virt: VirtAddr) -> Result<Self::Block> {
        TI::extract(virt)
        // convert to Result<Block>
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
