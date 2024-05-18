//# snafu = "*"
//# either = "*"
//
// Explore memory table abstractions
//
// #![feature(decl_macro)]
#![feature(allocator_api)]
#![allow(unused)]
#![allow(unused_imports)]
use {
    core::{
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
    either::*,
    snafu::{ResultExt, Snafu},
    std::alloc::{alloc_zeroed, dealloc, Layout},
};

type VirtAddr = u64;
type PhysAddr = u64;

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
    fn extract_block_base(entry: u64) -> u64 {
        entry & Self::BLOCK_ADDR_MASK
    }
    // Extract address of the next table (use by TableOnly) -- aligned to the granule size for VMSAv8
    fn extract_table_base(entry: u64) -> u64 {
        entry & Self::TABLE_ADDR_MASK
    }
}

// pub enum TraverseError {

#[derive(Debug, Clone, Copy, PartialEq)] // Snafu
enum TableError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    NotPresent,
}

trait TableOnly {
    type NextTable;
    fn next_table(&self, virt: VirtAddr) -> Result<&mut Self::NextTable, TableError>;
}

#[derive(Debug, Clone, Copy, PartialEq)] // Snafu
enum BlockError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    NotPresent,
}

trait BlockOnly {
    type Block;
    fn block(&self, virt: VirtAddr) -> Result<Self::Block, BlockError>;
}

trait TableOrBlock {
    type NextTable;
    fn next_table(&self, virt: VirtAddr) -> Result<&mut Self::NextTable, TableError>;
    type Block;
    fn block(&self, virt: VirtAddr) -> Result<Self::Block, BlockError>;
}

// @todo next() should also take a VirtAddr?
// trait NextStage: TableOrBlock {
//     fn next(
//         &self,
//     ) -> Either<dyn TableOnly<NextTable = Self::NextTable>, dyn BlockOnly<Block = Self::Block>>;
// }

macro_rules! make_stage {
    { name:$ident } => {
        #[allow(non_camel_case_types)]
        struct $name {
            entries: [u64; 1 << Self::MASK_BITS], // u64 must be more flexible type EntryT for other arch's
        }
    }
}

// As you can see, the Stage structures are repeated, as are the granules (masks),
// so we need to be able to parameterize them both somehow
// e.g. mask depends on both stage and granule
// These stage tables (or PageTable<Stage> really) can be implemented in terms of TableIndex::MASK_BITS
// e.g. struct Stage<I: TableIndex> { pub entries: [u64; 1 << I::MASK_BITS]; }
// Specific table structures:

make_stage!(Stage1_Gran4k);
make_stage!(Stage2_Gran4k);
make_stage!(Stage3_Gran4k);
make_stage!(Stage4_Gran4k);

make_stage!(Stage1_Gran16k);
make_stage!(Stage2_Gran16k);
make_stage!(Stage3_Gran16k);
make_stage!(Stage4_Gran16k);

make_stage!(Stage1_Gran64k);
make_stage!(Stage2_Gran64k);
make_stage!(Stage3_Gran64k);

macro_rules! impl_table_index {
    { $stage:ty, index = $mask_bits:literal @ $shift:literal, table = $table_bits:literal @ $table_shift:literal, block = $block_bits:literal @ $block_shift:literal } => {
        impl TableIndex for $stage {
            const MASK_BITS: usize = $mask_bits;
            const SHIFT: usize = $shift;
            const BLOCK_ADDR_MASK: u64 = ((1 << $block_bits) - 1) << $block_shift;
            const TABLE_ADDR_MASK: u64 = ((1 << $table_bits) - 1) << $table_shift;
        }
    }
}

impl_table_index!(Stage1_Gran4k, index = 9@39, table = 36@12, block = 0@0);
impl_table_index!(Stage2_Gran4k, index = 9@30, table = 36@12, block = 18@30);
impl_table_index!(Stage3_Gran4k, index = 9@21, table = 36@12, block = 27@21);
impl_table_index!(Stage4_Gran4k, index = 9@12, table = 36@12, block = 36@12);

impl_table_index!(Stage1_Gran16k, index = 1@47, table = 34@14, block = 0@0);
impl_table_index!(Stage2_Gran16k, index = 11@36, table = 34@14, block = 12@36);
impl_table_index!(Stage3_Gran16k, index = 11@25, table = 34@14, block = 23@25);
impl_table_index!(Stage4_Gran16k, index = 11@14, table = 34@14, block = 34@14);

impl_table_index!(Stage1_Gran64k, index = 5@42, table = 32@16, block = 5@42); // 4TiB block!
impl_table_index!(Stage2_Gran64k, index = 13@29, table = 32@16, block = 19@29);
impl_table_index!(Stage3_Gran64k, index = 13@16, table = 32@16, block = 32@16);
// impl_table_index!(Stage4_Gran64k, index = 0@0); // N/A

unsafe fn next_level(virt: VirtAddr) -> Result<PhysAddr, TableError> {
    let index = <$stage>::extract_index(virt);
    let entry = self.entries[index];
    if !bit_set(entry, P) {
        return Err(TableError::NotPresent);
    }
    Ok(base)
}

macro_rules! impl_table_only {
    { $stage:ty, $next_stage:ty } => {
        impl TableOnly for $stage where $stage: TableIndex {
            type NextTable = $next_stage;

            fn next_table(&self, virt: VirtAddr) -> Result<&mut Self::NextTable, TableError> {
                let entry = next_level(virt);
                let base = <$stage>::extract_table_base(entry); // This involves some stage-dependent shenanigans, e.g. for RISC-V the PN[x] entries must be combined depending on the stage.
                let next_table = base as *mut Self::NextTable; // ptr.cast<>?
                let next_table = unsafe { &mut *next_table };
                Ok(next_table)
            }
        }
    }
}

// temp:
const P: usize = 0x0;

fn bit_set(field: u64, bit: usize) -> bool {
    field & (1 << bit) != 0
}

macro_rules! impl_block_only {
    { $stage:ty, $block_size:ty } => {
        impl BlockOnly for $stage where $stage: TableIndex {
            type Block = Frame<$block_size>; // we will get a block of given size from this stage

            fn block(&self, virt: VirtAddr) -> Result<Self::Block, BlockError> {
                let entry = next_level(virt);
                let phys_base = <$stage>::extract_block_base(entry);
                let block = Frame::new(phys_base.try_into().expect("It fits fine!"));
                Ok(block)
            }
        }
    }
}

// Granularity 4KiB
impl_table_only!(Stage1_Gran4k, Stage2_Gran4k);

impl_table_only!(Stage2_Gran4k, Stage3_Gran4k);
impl_block_only!(Stage2_Gran4k, Size1GiB);

impl_table_only!(Stage3_Gran4k, Stage4_Gran4k);
impl_block_only!(Stage3_Gran4k, Size2MiB);

impl_block_only!(Stage4_Gran4k, Size4KiB);

// Granularity 16KiB
impl_table_only!(Stage1_Gran16k, Stage2_Gran16k);

impl_table_only!(Stage2_Gran16k, Stage3_Gran16k);
impl_block_only!(Stage2_Gran16k, Size1GiB);

impl_table_only!(Stage3_Gran16k, Stage4_Gran16k);
impl_block_only!(Stage3_Gran16k, Size2MiB);

impl_block_only!(Stage4_Gran16k, Size4KiB);

// Granularity 64KiB
impl_table_only!(Stage1_Gran64k, Stage2_Gran64k);
impl_block_only!(Stage1_Gran64k, Size4TiB);

impl_table_only!(Stage2_Gran64k, Stage3_Gran64k);
impl_block_only!(Stage2_Gran64k, Size512MiB);

impl_block_only!(Stage3_Gran64k, Size64KiB);

macro_rules! impl_next_stage {
    { $stage:ty, $next_stage:ty, $block:ty } => {
        // @todo: Index for slicerange (restricted range! must maintain alignments)
        // or range to fill in sequential table blocks on aarch64
        // e.g. stage4[0..15] = compound_large_segment; <- not using the VirtAddr here..
        impl Index<VirtAddr> for $stage
        where
             $stage: TableOnly,
        {
            type Output = Self::NextTable;

            fn index(&self, virt: VirtAddr) -> &Self::Output {
                let tbl = <$stage as TableOnly>::next_table(virt);
                &*tbl
            }
        }

        impl IndexMut<VirtAddr> for &mut $stage
        where
             $stage: TableOnly,
        {
            fn index_mut(&mut self, virt: VirtAddr) -> &mut Self::Output {
                let index = Self::extract_index(virt);
                Self::extract_table_base(self.entries[index])
            }
        }

        impl Index<VirtAddr> for &$stage
        where
            $stage: BlockOnly,
        {
            type Output = <Self as BlockOnly>::Block;

            fn index(&self, virt: VirtAddr) -> &Self::Output {
                let index = Self::extract_index(virt);
                Self::extract_block_base(self.entries[index])
            }
        }

        impl IndexMut<VirtAddr> for &mut $stage
        where
            $stage: BlockOnly,
        {
            // Should only return a mutable reference to entries[index] so that we
            // could replace it.
            fn index_mut(&mut self, virt: VirtAddr) -> &mut Self::Output {
                let index = Self::extract_index(virt);
                &mut self.entries[index] // but can't be just &mut u64, we need a proper table type? probably need to overload assignments too?
            }
        }

        // impl<T> Index<VirtAddr> for T
        // where
        //     T: TableIndex + TableOrBlock,
        // {
        //     type Output = Either<T::NextTable, T::Block>;
        //     fn index(&self, index: VirtAddr) -> &Self::Output {
        //         self.entries[index]
        //     }
        // }
    }
}

// now for the TableOnly+BlockOnly combination we need to also provide a resolution method that returns either
impl_next_stage!(Stage1_Gran4k, Stage2_Gran4k, Size1GiB); // this provides next() -> Either<Table,Block>
impl_next_stage!(Stage2_Gran4k, Stage3_Gran4k, Size2MiB);

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

macro_rules! make_page {
    { $name:ident, $debug_str:literal, $shift:literal } => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
        pub enum $name {}

        impl PageSize for $name {
            const SIZE_AS_DEBUG_STR: &'static str = $debug_str;
            const SHIFT: usize = $shift;
        }
    }
}

//------------------------
// Page: with 4kb granule
//------------------------

// A standard 4KiB page.
make_page!(Size4KiB, "4KiB", 12);

//-------------------------
// Page: with 16kb granule
//-------------------------

// A standard 16KiB page.
make_page!(Size16KiB, "16KiB", 14);

//-------------------------
// Page: with 64kb granule
//-------------------------

// A standard 64KiB page.
make_page!(Size64KiB, "64KiB", 16);

//--------------------------
// Blocks: with 4kb granule
//--------------------------

// A “huge” 2MiB page.
make_page!(Size2MiB, "2MiB", 21);

// A “giant” 1GiB page.
make_page!(Size1GiB, "1GiB", 30);

//---------------------------
// Blocks: with 16kb granule
//---------------------------

make_page!(Size32MiB, "32MiB", 25);

//---------------------------
// Blocks: with 64kb granule
//---------------------------

make_page!(Size512MiB, "512MiB", 29);

make_page!(Size4TiB, "4TiB", 42);

/// Physical page frame.
struct Frame<P: PageSize> {
    base: usize,
    _page_size: PhantomData<P>,
}

impl<P: PageSize> Frame<P> {
    // @todo Check base is aligned to _page_size::SHIFT
    pub fn new(base: usize) -> Self {
        Self {
            base,
            _page_size: PhantomData,
        }
    }
}

struct PageTableAllocator;
struct FrameAllocator;

#[derive(Debug, Snafu)]
enum AllocError {
    InvalidLayout { source: std::alloc::LayoutError },
}

impl PageTableAllocator {
    fn alloc<T: TableIndex>() -> Result<&mut T, AllocError> {
        let size = (1 << T::MASK_BITS) * core::mem::size_of::<u64>();
        let layout = Layout::from_size_align(size, size).context(InvalidLayoutSnafu)?;
        println!(
            "Allocating page table: size {}, align {}",
            layout.size(),
            layout.align()
        );
        let ptr = unsafe { alloc_zeroed(layout) };
        assert_ne!(ptr as usize, 0);
        Ok(ptr as *mut T)
    }
    fn dealloc<T: TableIndex>(ptr: *mut T) -> Result<(), AllocError> {
        let size = (1 << T::MASK_BITS) * core::mem::size_of::<u64>();
        let layout = Layout::from_size_align(size, size).context(InvalidLayoutSnafu)?;
        unsafe { dealloc(ptr as *mut u8, layout) };
        Ok(())
    }
}

impl FrameAllocator {
    fn alloc<P: PageSize>() -> Result<Frame<P>, AllocError> {
        let layout =
            Layout::from_size_align(1 << P::SHIFT, 1 << P::SHIFT).context(InvalidLayoutSnafu)?;
        println!(
            "Allocating frame: size {}, align {}",
            layout.size(),
            layout.align()
        );
        let ptr = unsafe { alloc_zeroed(layout) };
        assert_ne!(ptr as usize, 0);
        Ok(Frame::new(ptr as usize))
    }
    fn dealloc<P: PageSize>(ptr: Frame<P>) -> Result<(), AllocError> {
        let layout =
            Layout::from_size_align(1 << P::SHIFT, 1 << P::SHIFT).context(InvalidLayoutSnafu)?;
        unsafe { dealloc(ptr.base as *mut u8, layout) };
        Ok(())
    }
}

fn main() -> Result<(), AllocError> {
    println!("Hello, play");

    println!("Stage1_Gran4k {}", core::mem::size_of::<Stage1_Gran4k>());
    println!("Stage2_Gran4k {}", core::mem::size_of::<Stage2_Gran4k>());
    println!("Stage3_Gran4k {}", core::mem::size_of::<Stage3_Gran4k>());
    println!("Stage4_Gran4k {}", core::mem::size_of::<Stage4_Gran4k>());

    println!("Stage1_Gran16k {}", core::mem::size_of::<Stage1_Gran16k>());
    println!("Stage2_Gran16k {}", core::mem::size_of::<Stage2_Gran16k>());
    println!("Stage3_Gran16k {}", core::mem::size_of::<Stage3_Gran16k>());
    println!("Stage4_Gran16k {}", core::mem::size_of::<Stage4_Gran16k>());

    println!("Stage1_Gran64k {}", core::mem::size_of::<Stage1_Gran64k>());
    println!("Stage2_Gran64k {}", core::mem::size_of::<Stage2_Gran64k>());
    println!("Stage3_Gran64k {}", core::mem::size_of::<Stage3_Gran64k>());
    println!("Stage4_Gran64k {}", core::mem::size_of::<Stage4_Gran64k>());

    // Build page table hierarchy from Stage1 down to Stage4.
    let virt: VirtAddr = 0xdead_beef;

    // Using Granule4k:
    let s1_table = PageTableAllocator::alloc::<Stage1_Gran4k>()?;
    let s2_table = PageTableAllocator::alloc::<Stage2_Gran4k>()?;
    s1_table[virt] = s2_table;
    let s3_table = PageTableAllocator::alloc::<Stage3_Gran4k>()?;
    s2_table[virt] = s3_table;
    let s4_table = PageTableAllocator::alloc::<Stage4_Gran4k>()?;
    s3_table[virt] = s4_table;
    let leaf = FrameAllocator::alloc::<Size4KiB>()?;
    s4_table[virt] = leaf;

    // Using Granule16k:
    let mut s1_table = PageTableAllocator::alloc::<Stage1_Gran16k>()?;
    let mut s2_table = PageTableAllocator::alloc::<Stage2_Gran16k>()?;
    s1_table[virt] = s2_table;
    let mut s3_table = PageTableAllocator::alloc::<Stage3_Gran16k>()?;
    s2_table[virt] = s3_table;
    let mut s4_table = PageTableAllocator::alloc::<Stage4_Gran16k>()?;
    s3_table[virt] = s4_table;
    let leaf = FrameAllocator::alloc::<Size16KiB>()?;
    s4_table[virt] = leaf;

    // This should fail to compile (accidentally wrong stage granule)
    let mut s3_table = PageTableAllocator::alloc::<Stage3_Gran4k>()?;
    s2_table[virt] = s3_table;

    // This too (accidentally wrong table level)
    let s3_table = PageTableAllocator::alloc::<Stage4_Gran16k>()?;
    s2_table[virt] = s3_table;

    // // Using Granule64k:
    let s1_table = PageTableAllocator::alloc::<Stage1_Gran64k>()?;
    let s2_table = PageTableAllocator::alloc::<Stage2_Gran64k>()?;
    s1_table[virt] = s2_table;
    let s3_table = PageTableAllocator::alloc::<Stage3_Gran64k>()?;
    s2_table[virt] = s3_table;
    let leaf = FrameAllocator::alloc::<Size64KiB>()?;
    s3_table[virt] = leaf;

    //=================
    // TRAVERSE TABLES
    //=================

    let virt: VirtAddr = 0xdead_beef;

    // let tt_base_addr = 124467000usize; // this comes from TTBRx register or some table base variable for each process.

    // // everything starts with a translation table base address
    // let sys_l0_table = base_addr as *const Stage1<G>;

    // // Access the table using virtual addresses (each stage consumes more bits of the address).
    // let l0_table = sys_l0_table;
    // let l1 = l0_table[virt_addr];
    // let l2 = l1_table[virt_addr];
    // let phys = l2_table[virt_addr];

    // // to access physical memory from kernel
    // let phys = 123456usize;
    // let kern_phys = phys.phys_to_kernel();
    Ok(())
}
