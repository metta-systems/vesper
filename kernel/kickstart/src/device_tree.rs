//! Kickstart's boot device tree index. Parsing lives in `libdevicetree`.
//!
//! At EL2 Kickstart indexes the blob the firmware passed ([`index`]). At EL1
//! the identity map is gone, so [`at_el1`] indexes it again through the
//! kernel's physical window, once, from the location `BOOT_INFO` records.

use {
    crate::{
        boot_info::BOOT_INFO,
        memory::{Alloc, BootAllocator},
    },
    core::{
        cell::UnsafeCell,
        sync::atomic::{AtomicBool, Ordering},
    },
    libaddress::PhysAddr,
    libdevicetree::DeviceTree,
    liblocking::interface::Mutex,
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
pub(crate) unsafe fn index(
    dtb: *const u8,
    allocator: &mut BootAllocator,
) -> DeviceTree<'static, 'static> {
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

/// Bytes reserved for the EL1 device tree index.
const INDEX_BYTES: usize = 128 * 1024;

/// The EL1 index: its buffer and, once built, the tree.
#[repr(C, align(16))]
struct El1Index {
    buffer: UnsafeCell<[u8; INDEX_BYTES]>,
    tree: UnsafeCell<Option<DeviceTree<'static, 'static>>>,
}

// SAFETY: built once, guarded by `INDEXING`, on the single boot core; read
// only afterwards.
unsafe impl Sync for El1Index {}

static EL1_INDEX: El1Index = El1Index {
    buffer: UnsafeCell::new([0; INDEX_BYTES]),
    tree: UnsafeCell::new(None),
};
static INDEXING: AtomicBool = AtomicBool::new(false);

/// The firmware's device tree, indexed at EL1 through the kernel's physical
/// window. Kickstart keeps the blob reserved for the whole boot; the index is
/// built on the first call.
///
/// # Panics
///
/// If no blob was recorded, or it no longer parses.
pub fn at_el1() -> &'static DeviceTree<'static, 'static> {
    if !INDEXING.swap(true, Ordering::AcqRel) {
        let blob = BOOT_INFO.lock(|boot_info| boot_info.device_tree);
        assert_ne!(blob.size, 0, "🥾 no device tree recorded");
        let pointer = PhysAddr::new(blob.phys).user_to_kernel().as_ptr::<u8>();
        // SAFETY: the blob stays reserved and unmodified for the whole boot,
        // and the kernel's physical window maps all RAM.
        let raw = unsafe { DeviceTree::blob_from_raw(pointer) }
            .unwrap_or_else(|error| panic!("🥾 device tree header: {error:?}"));
        let layout = DeviceTree::index_layout(&raw)
            .unwrap_or_else(|error| panic!("🥾 device tree index layout: {error:?}"));
        assert!(
            layout.size() + layout.align() <= INDEX_BYTES,
            "🥾 the device tree index needs {} bytes",
            layout.size() + layout.align()
        );
        // SAFETY: `INDEXING` hands the buffer out once, for the rest of the
        // boot; nothing reads the tree before it is stored.
        unsafe {
            let tree = DeviceTree::new(raw, &mut *EL1_INDEX.buffer.get())
                .unwrap_or_else(|error| panic!("🥾 device tree index: {error:?}"));
            *EL1_INDEX.tree.get() = Some(tree);
        }
    }
    // SAFETY: stored above on the first call; never written again.
    unsafe { (*EL1_INDEX.tree.get()).as_ref() }.expect("🥾 the device tree is being indexed")
}
