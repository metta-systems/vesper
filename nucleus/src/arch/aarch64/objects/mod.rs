/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

mod asid_control;
mod asid_pool;
mod page;
mod page_directory;
mod page_global_directory;
mod page_table;
mod page_upper_directory;

// Allocation details

// 1. should be possible to map non-SAS style
// 2. should be easy to map SAS style
// 3. should not allocate any memory dynamically
//    ^ problem with the above API is FrameAllocator
//    ^ clients should supply their own memory for frames... from FrameCaps

// https://github.com/seL4/seL4_libs/tree/master/libsel4allocman

// Allocation overview

// Allocation is complex due to the circular dependencies that exist on allocating resources. These dependencies are loosely described as

//     Capability slots: Allocated from untypeds, book kept in memory.
//     Untypeds / other objects (including frame objects): Allocated from other untypeds, into capability slots, book kept in memory.
//     memory: Requires frame object.

//=============================================================================

// ActivePageTable (--> impl VirtSpace for ActivePageTable etc...)
// * translate(VirtAddr)->PhysAddr
// * translate_page(Page)->PhysAddr
// * map_to(Page, PhysFrame, Flags, FrameAllocator)->()
// * map(Page, Flags, FrameAllocator)->()
// * identity_map(PhysFrame, Flags, FrameAllocator)->()
// * unmap(Page, FrameAllocator)->()

trait VirtSpace {
    fn map(virt_space: VirtSpace/*Cap*/, vaddr: VirtAddr, rights: CapRights, attr: VMAttributes) -> Result<()>; /// ??
    fn unmap() -> Result<()>; /// ??
    fn remap(virt_space: VirtSpace/*Cap*/, rights: CapRights, attr: VMAttributes) -> Result<()>; /// ??
    fn get_address() -> Result<PhysAddr>;///??
}

// ARM AArch64 processors have a four-level page-table structure, where the
// VirtSpace is realised as a PageGlobalDirectory. All paging structures are
// indexed by 9 bits of the virtual address.

// AArch64 page hierarchy:
//
// PageGlobalDirectory (L0)  -- aka VirtSpace
// +--PageUpperDirectory (L1)
//    +--Page<Size1GiB> -- aka HugePage
//    |  or
//    +--PageDirectory (L2)
//       +--Page<Size2MiB> -- aka LargePage
//       |  or
//       +--PageTable (L3)
//          +--Page<Size4KiB> -- aka Page


/// Cache data management.
trait PageCacheManagement {
    /// Cleans the data cache out to RAM.
    /// The start and end are relative to the page being serviced.
    fn clean_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Clean and invalidates the cache range within the given page.
    /// The range will be flushed out to RAM. The start and end are relative
    /// to the page being serviced.
    fn clean_invalidate_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Invalidates the cache range within the given page.
    /// The start and end are relative to the page being serviced and should
    /// be aligned to a cache line boundary where possible. An additional
    /// clean is performed on the outer cache lines if the start and end are
    /// not aligned, to clean out the bytes between the requested and
    /// the cache line boundary.
    fn invalidate_data(start_offset: usize, end_offset: usize) -> Result<()>;
    /// Cleans data lines to point of unification, invalidates
    /// corresponding instruction lines to point of unification, then
    /// invalidates branch predictors.
    /// The start and end are relative to the page being serviced.
    fn unify_instruction_cache(start_offset: usize, end_offset: usize) -> Result<()>;
}
