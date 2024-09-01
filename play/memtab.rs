//# snafu = "*"
// Explore memory table abstractions
//
//
use {
    core::{
        marker::PhantomData,
        ops::{Index, IndexMut},
    },
    snafu::Snafu,
};

fn main() {
    println!("Hello, play");

    // Build page table hierarchy from Stage1 down to Stage4.
    // Using Granule4k:
    // Using Granule16k:
    let l0_table = 0;

    // Using Granule64k:
}

// impl indexing stages by virtual address only,
// so l0[virt][virt][virt][virt] will go through all levels of translation
//

impl<G: GranuleSize> Index<Virtual> for Stage1<G> {
    type Output = Self::NextStage;

    fn index(&self, index: Virtual) -> Self::NextStage {}
}

type Physical = u64;
type Virtual = u64;

trait Stage {
    type G: Granule;
    type NextStage;
}

trait Descriptor {
    type GRANULE: Granule;
    type STAGE: Stage<G = Self::GRANULE>; // Get NextStage from this STAGE
    fn is_leaf() -> bool;
}

trait NextLevelDescriptor: Descriptor {
    fn get_next_level_desciptor() -> impl Descriptor;
}

trait LeafDescriptor: Descriptor {
    fn get_translated_address() -> Physical;
}

/// Abstract over possible granule sizes.
trait Granule {
    // Mask and shift to extract entry index from virt address
    const MASK_BITS: usize;
    const MASK: u64 = 1 << Self::MASK_BITS - 1;
    const SHIFT: usize;
    fn get_index(address: Virtual) -> usize {
        return ((address >> Self::SHIFT) & Self::MASK)
            .try_into()
            .expect("Arithmetics gone mad");
    }
}

/// Abstract over the possible page sizes, 4KiB, 16KiB, 2MiB, 1GiB.
/// Page sizes depend on stage and granule size - could be auto-derived?
pub trait PageSize: Copy + PartialEq + Eq + PartialOrd + Ord {
    /// The page size in bytes.
    const SIZE: usize = 1 << Self::SHIFT;

    /// A string representation of the page size for debug output.
    const SIZE_AS_DEBUG_STR: &'static str;

    /// The page shift in bits.
    const SHIFT: usize;

    /// The page mask in bits.
    const MASK: u64 = 1 << Self::SHIFT - 1;

    fn alignment() -> usize {
        Self::SIZE
    }

    fn mask() -> u64 {
        Self::MASK
    }
}

/// This trait is implemented for 4KiB, 16KiB, and 2MiB pages, but not for 1GiB pages.
/// This trait is actually not necessary - do the BlockSize trait instead.
trait NotGiantPageSize: PageSize {}

/// Marker for granule sizes impls.
trait GranuleSize {}

/// Stages are parameterised by the used granule size. This determines their max size and resolution step.
/// It also determines the return type (table/block/page) of the resolution step? hmm.
/// (use an assoc type to resolve it e.g. trait NextStage { type Resolution; fn resolve() -> Self::Resolution; })
struct Stage1<G: GranuleSize> {
    _granule: PhantomData<G>,
}
struct Stage2<G: GranuleSize> {
    _granule: PhantomData<G>,
}
struct Stage3<G: GranuleSize> {
    _granule: PhantomData<G>,
}
struct Stage4<G: GranuleSize> {
    _granule: PhantomData<G>,
}

trait NextStage {
    type BaseTable; // not Stage1..4 b/c we can't pass any stage to a stage 1
    type Resolution;
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution;
}

// @todo: impl_granule! macro?
impl NextStage for Stage1<Granule4k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage1<Granule16k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage1<Granule64k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}

impl NextStage for Stage2<Granule4k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage2<Granule16k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage2<Granule64k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}

impl NextStage for Stage3<Granule4k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage3<Granule16k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage3<Granule64k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}

impl NextStage for Stage4<Granule4k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage4<Granule16k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}
impl NextStage for Stage4<Granule64k> {
    type BaseTable = u64; // ?
    type Resolution = u64; // ?
    fn resolve(_table: &Self::BaseTable) -> Self::Resolution {
        0u64
    }
}

// Stage 1 always points to more tables
// impl NextLevelDescriptor for Stage1 {
//     fn get_next_level_descriptor(table: &AssociatedTranslationTable) -> impl Descriptor {
//         // check validity bits
//         // return next descriptor from the associated table
//     }
// }

// impl NextLevelDescriptor for Stage2 {}

// impl NextLevelDescriptor for Stage3 {}

// impl LeafDescriptor for Stage4 {}

//
// Probably implement it as Granules vs Stages?
// Some systems may support variable granule sizes, some may not.
// Each Stage/Granule combination will yield bitmasks and/or bitfield! references
// and/or types of further stages (shall return enums?)
//

// Specific granules

struct Granule4k;
struct Granule16k;
struct Granule64k;

impl GranuleSize for Granule4k {}
impl GranuleSize for Granule16k {}
impl GranuleSize for Granule64k {}

// Masks to extract entry index from virt address for different stages and granule sizes
impl Granule for Stage1<Granule4k> {
    const MASK_BITS: usize = 9;
    const SHIFT: usize = 39;
}
impl Granule for Stage1<Granule16k> {
    const MASK_BITS: usize = 1;
    const SHIFT: usize = 47;
}
impl Granule for Stage1<Granule64k> {
    const MASK_BITS: usize = 5;
    const SHIFT: usize = 42;
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

impl NotGiantPageSize for Size4KiB {}

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

impl NotGiantPageSize for Size16KiB {}

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

impl NotGiantPageSize for Size64KiB {}

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

impl NotGiantPageSize for Size2MiB {}

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

impl NotGiantPageSize for Size32MiB {}

//---------------------------
// Blocks: with 64kb granule
//---------------------------

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size512MiB {}

impl PageSize for Size512MiB {
    const SIZE_AS_DEBUG_STR: &'static str = "512MiB";
    const SHIFT: usize = 29;
}

impl NotGiantPageSize for Size512MiB {}

/// The error returned by the `PageTableEntry::frame` method.
#[derive(Snafu, Debug, Clone, Copy, PartialEq)]
pub enum FrameError {
    /// The entry does not have the `PRESENT` flag set, so it isn't currently mapped to a frame.
    FrameNotPresent,
    /// The entry has the `HUGE_PAGE` flag set. The `frame` method has a standard 4KiB frame
    /// as return type, so a huge frame can't be returned. @todo
    HugeFrame,
}
