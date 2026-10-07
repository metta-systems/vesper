//! Page-table walking and the retained init image's geometry.

use {kickstart::bootstrap::RetainedInitMemory, libaddress::PhysAddr};

pub const PAGE: u64 = 4096;
/// The span one L3 table covers.
pub const LEAF_SPAN: u64 = 2 * 1024 * 1024;
pub const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;

/// Walk a retained root to the descriptor translating `vaddr`, without
/// holding references across SVCs. Returns the level and descriptor of the
/// leaf, or `None` if a level has no valid descriptor.
pub fn find_leaf(ttbr: u64, vaddr: u64) -> Option<(u32, u64)> {
    let mut table_paddr = ttbr & ADDR_MASK;
    for level in 0..4 {
        let slot = usize::try_from((vaddr >> (39 - 9 * level)) & 0x1ff).ok()?;
        // SAFETY: callers supply retained roots; every subsequent page is a
        // valid table descriptor in accounted private backing. Slots are 9-bit.
        let entry = unsafe {
            PhysAddr::new(table_paddr)
                .user_to_kernel()
                .as_ptr::<u64>()
                .add(slot)
                .read_volatile()
        };
        if entry & 1 == 0 {
            return None;
        }
        if level == 3 {
            assert_eq!(entry & 3, 3, "L3 descriptors must be page leaves");
            return Some((level, entry));
        }
        if entry & 2 == 0 {
            assert_ne!(level, 0, "L0 cannot hold a block");
            return Some((level, entry));
        }
        table_paddr = entry & ADDR_MASK;
    }
    unreachable!("four-level walk must reach a leaf")
}

/// [`find_leaf`] for a mapping that must exist.
pub fn read_leaf(ttbr: u64, vaddr: u64) -> (u32, u64) {
    find_leaf(ttbr, vaddr).unwrap_or_else(|| panic!("missing descriptor for {vaddr:#x}"))
}

/// Number of L3 tables (2 MiB spans) the retained image and low stack need
/// per root, checking the linked layout the fixtures expect.
pub fn image_table_count(retained: &RetainedInitMemory) -> usize {
    let (start, end) = retained.image();
    let (stack_start, stack_end) = retained.stack();
    assert_eq!(start, 0x80000, "fixture expects the linked init base");
    assert_eq!(stack_start, PAGE);
    assert_eq!(stack_end, start);
    let count = usize::try_from(end.div_ceil(LEAF_SPAN)).unwrap_or(usize::MAX);
    assert!(
        count <= 8,
        "image exceeds the fixture's eight L3 slots per root"
    );
    count
}
