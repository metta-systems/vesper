//! Kickstart's boot device tree index. Parsing lives in `libdevicetree`.

use {
    crate::memory::{Alloc, BootAllocator},
    libdevicetree::DeviceTree,
};

/// Index the device tree at `dtb` in a droppable boot allocation.
///
/// # Safety
///
/// `dtb` must point at the flattened device tree the boot firmware passed,
/// left intact for the rest of the boot.
///
/// # Panics
///
/// If the blob is malformed or its index cannot be allocated.
pub unsafe fn index(dtb: *const u8, allocator: &mut BootAllocator) -> DeviceTree<'static, 'static> {
    // SAFETY: the caller's contract.
    let blob = unsafe { DeviceTree::blob_from_raw(dtb) }.expect("🥾 DeviceTree failed to read");
    let layout = DeviceTree::index_layout(&blob).expect("🥾 Couldn't calculate DeviceTree index");
    let block = allocator
        .alloc_aligned(
            layout.size(),
            layout.align(),
            ("DTB index", Alloc::Droppable),
        )
        .expect("🥾 Couldn't allocate DeviceTree index");
    // SAFETY: a fresh, exclusively owned boot allocation of `layout.size()`
    // bytes that stays allocated for the rest of the boot.
    let buffer = unsafe { core::slice::from_raw_parts_mut(block.as_mut_ptr(), layout.size()) };
    DeviceTree::new(blob, buffer).expect("🥾 Couldn't initialize indexed DeviceTree")
}
