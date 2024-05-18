/// Trait for abstracting over the possible page sizes, 4KiB, 16KiB, 2MiB, 1GiB.
pub trait PageSize: Copy + PartialEq + Eq + PartialOrd + Ord {
    /// The page size in bytes.
    const SIZE: usize;

    /// A string representation of the page size for debug output.
    const SIZE_AS_DEBUG_STR: &'static str;

    /// The page shift in bits.
    const SHIFT: usize;

    /// The page mask in bits.
    const MASK: u64;
}

/// This trait is implemented for 4KiB, 16KiB, and 2MiB pages, but not for 1GiB pages.
pub trait NotGiantPageSize: PageSize {} // @todo doesn't have to be pub??

/// A standard 4KiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size4KiB {}

impl PageSize for Size4KiB {
    const SIZE: usize = 4 * 1024;
    const SIZE_AS_DEBUG_STR: &'static str = "4KiB";
    const SHIFT: usize = 12;
    const MASK: u64 = 0xfff;
}

impl NotGiantPageSize for Size4KiB {}

/// A standard 16KiB page.
/// Currently unused.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size16KiB {}

impl PageSize for Size16KiB {
    const SIZE: usize = 16 * 1024;
    const SIZE_AS_DEBUG_STR: &'static str = "16KiB";
    const SHIFT: usize = 14;
    const MASK: u64 = 0x3fff;
}

impl NotGiantPageSize for Size16KiB {}

/// A “huge” 2MiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size2MiB {}

impl PageSize for Size2MiB {
    const SIZE: usize = 2 * 1024 * 1024;
    const SIZE_AS_DEBUG_STR: &'static str = "2MiB";
    const SHIFT: usize = 21;
    const MASK: u64 = 0x1f_ffff;
}

impl NotGiantPageSize for Size2MiB {}

/// A “giant” 1GiB page.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size1GiB {}

impl PageSize for Size1GiB {
    const SIZE: usize = 1024 * 1024 * 1024;
    const SIZE_AS_DEBUG_STR: &'static str = "1GiB";
    const SHIFT: usize = 30;
    const MASK: u64 = 0x3fff_ffff;
}
