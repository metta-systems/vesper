//! Platform memory management unit.

use {
    crate::{
        Physical, Virtual,
        mmu::{
            AddressSpace, AssociatedTranslationTable, MemoryRegion, PageAddress, TranslationGranule,
        },
    },
    liblocking::InitStateLock,
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

type KernelTranslationTable =
    <KernelVirtAddrSpace as AssociatedTranslationTable>::TableStartFromBottom;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// The translation granule chosen by this platform. This will be used everywhere else
/// in the kernel to derive respective data structures and their sizes.
/// For example, the `crate::memory::mmu::Page`.
pub type KernelGranule = TranslationGranule<{ 64 * 1024 }>;

/// The kernel's virtual address space defined by this platform.
pub type KernelVirtAddrSpace = AddressSpace<{ 1024 * 1024 * 1024 }>;

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

/// The kernel translation tables.
///
/// It is mandatory that `InitStateLock` is transparent.
/// That is, `size_of(InitStateLock<KernelTranslationTable>) == size_of(KernelTranslationTable)`.
/// There is a unit tests that checks this property.
pub static KERNEL_TABLES: InitStateLock<KernelTranslationTable> =
    InitStateLock::new(KernelTranslationTable::new());

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

/// Helper function for calculating the number of pages the given parameter spans.
const fn size_to_num_pages(size: usize) -> usize {
    assert!(size > 0);
    assert!(size.is_multiple_of(KernelGranule::SIZE)); // assert! is const-fn-friendly

    size >> KernelGranule::SHIFT
}

/// The code pages of the kernel binary.
pub fn virt_code_region() -> MemoryRegion<Virtual> {
    let num_pages = size_to_num_pages(super::code_size());

    let start_page_addr = super::virt_code_start();
    let end_exclusive_page_addr = start_page_addr
        .checked_offset(num_pages.cast_signed())
        .unwrap();

    MemoryRegion::new(start_page_addr, end_exclusive_page_addr)
}

/// The data pages of the kernel binary.
pub fn virt_data_region() -> MemoryRegion<Virtual> {
    let num_pages = size_to_num_pages(super::data_size());

    let start_page_addr = super::virt_data_start();
    let end_exclusive_page_addr = start_page_addr
        .checked_offset(num_pages.cast_signed())
        .unwrap();

    MemoryRegion::new(start_page_addr, end_exclusive_page_addr)
}

/// The boot core stack pages.
pub fn virt_boot_core_stack_region() -> MemoryRegion<Virtual> {
    let num_pages = size_to_num_pages(super::boot_core_stack_size());

    let start_page_addr = super::virt_boot_core_stack_start();
    let end_exclusive_page_addr = start_page_addr
        .checked_offset(num_pages.cast_signed())
        .unwrap();

    MemoryRegion::new(start_page_addr, end_exclusive_page_addr)
}

// The binary is still identity mapped, so use this trivial conversion function for mapping below.

fn kernel_virt_to_phys_region(virt_region: MemoryRegion<Virtual>) -> MemoryRegion<Physical> {
    MemoryRegion::new(
        PageAddress::from(virt_region.start_page_addr().into_inner().as_usize()),
        PageAddress::from(
            virt_region
                .end_exclusive_page_addr()
                .into_inner()
                .as_usize(),
        ),
    )
}

//--------------------------------------------------------------------------------------------------
// Subsumed by the kernel_map_binary() function
//--------------------------------------------------------------------------------------------------

// These are part of a static linked image and used for proper kernel-space initialization.
// i.e. these data are subtracted from the dtb-provided memory map.
// pub static LAYOUT: KernelVirtualLayout<NUM_MEM_RANGES> = KernelVirtualLayout::new(
//     memory_map::END_INCLUSIVE,
//     [
//         TranslationDescriptor {
//             name: "Remapped Device MMIO",
//             virtual_range: remapped_mmio_range_inclusive,
//             physical_range_translation: Translation::Offset(
//                 memory_map::mmio::MMIO_BASE + 0x20_0000,
//             ),
//             attribute_fields: AttributeFields {
//                 mem_attributes: MemAttributes::Device,
//                 acc_perms: AccessPermissions::ReadWrite,
//                 execute_never: true,
//             },
//         },
// @todo these should come from DTB and mem-map?
//         TranslationDescriptor {
//             name: "Device MMIO",
//             virtual_range: mmio_range_inclusive,
//             physical_range_translation: Translation::Identity,
//             attribute_fields: AttributeFields {
//                 mem_attributes: MemAttributes::Device,
//                 acc_perms: AccessPermissions::ReadWrite,
//                 execute_never: true,
//             },
//         },
// @todo these should come from DTB and mem-map?
//         TranslationDescriptor {
//             name: "DMA heap pool",
//             virtual_range: dma_range_inclusive,
//             physical_range_translation: Translation::Identity,
//             attribute_fields: AttributeFields {
//                 mem_attributes: MemAttributes::NonCacheableDRAM,
//                 acc_perms: AccessPermissions::ReadWrite,
//                 execute_never: true,
//             },
//         },
//         TranslationDescriptor {
//             name: "Framebuffer area (static for now)",
//             virtual_range: || {
//                 RangeInclusive::new(
//                     memory_map::phys::VIDEOMEM_BASE,
//                     memory_map::mmio::MMIO_BASE - 1,
//                 )
//             },
//             physical_range_translation: Translation::Identity,
//             attribute_fields: AttributeFields {
//                 mem_attributes: MemAttributes::Device,
//                 acc_perms: AccessPermissions::ReadWrite,
//                 execute_never: true,
//             },
//         },
//     ],
// );

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return a reference to the kernel's translation tables.
pub fn kernel_translation_tables() -> &'static InitStateLock<KernelTranslationTable> {
    &KERNEL_TABLES
}

/// The MMIO remap pages.
pub fn virt_mmio_remap_region() -> MemoryRegion<Virtual> {
    let num_pages = size_to_num_pages(super::mmio_remap_size());

    let start_page_addr = super::virt_mmio_remap_start();
    let end_exclusive_page_addr = start_page_addr
        .checked_offset(num_pages.cast_signed())
        .unwrap();

    MemoryRegion::new(start_page_addr, end_exclusive_page_addr)
}

/// Map the kernel binary.
///
/// # Safety
///
/// - Any miscalculation or attribute error will likely be fatal. Needs careful manual checking.
pub unsafe fn kernel_map_binary() -> Result<(), &'static str> {
    // SAFETY: Make a mistake and you're dead, gaijin!
    // unsafe {
    //     generic_mmu::kernel_map_at(
    //         "Kernel boot-core stack",
    //         &virt_boot_core_stack_region(),
    //         &kernel_virt_to_phys_region(virt_boot_core_stack_region()),
    //         AttributeFields {
    //             mem_attributes: MemAttributes::CacheableDRAM,
    //             acc_perms: AccessPermissions::ReadWrite,
    //             execute_never: true,
    //         },
    //     )?;
    // }

    //         TranslationDescriptor {
    //             name: "Boot code and data",
    //             virtual_range: boot_range_inclusive,
    //             physical_range_translation: Translation::Identity,
    //             attribute_fields: AttributeFields {
    //                 mem_attributes: MemAttributes::CacheableDRAM,
    //                 acc_perms: AccessPermissions::ReadOnly,
    //                 execute_never: false,
    //             },
    //         },

    //         TranslationDescriptor {
    //             name: "Kernel code and RO data",
    //             virtual_range: code_range_inclusive,
    //             physical_range_translation: Translation::Identity,
    //             attribute_fields: AttributeFields {
    //                 mem_attributes: MemAttributes::CacheableDRAM,
    //                 acc_perms: AccessPermissions::ReadOnly,
    //                 execute_never: false,
    //             },
    //         },

    // SAFETY: Make a mistake and you're dead, gaijin!
    // unsafe {
    //     generic_mmu::kernel_map_at(
    //         "Kernel code and RO data",
    //         &virt_code_region(),
    //         &kernel_virt_to_phys_region(virt_code_region()),
    //         AttributeFields {
    //             mem_attributes: MemAttributes::CacheableDRAM,
    //             acc_perms: AccessPermissions::ReadOnly,
    //             execute_never: false,
    //         },
    //     )?;
    // }

    // SAFETY: Make a mistake and you're dead, gaijin!
    // unsafe {
    //     generic_mmu::kernel_map_at(
    //         "Kernel data and bss",
    //         &virt_data_region(),
    //         &kernel_virt_to_phys_region(virt_data_region()),
    //         AttributeFields {
    //             mem_attributes: MemAttributes::CacheableDRAM,
    //             acc_perms: AccessPermissions::ReadWrite,
    //             execute_never: true,
    //         },
    //     )?;
    // }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

// fn boot_range_inclusive() -> RangeInclusive<usize> {
//     RangeInclusive::new(super::boot_start(), super::boot_end_exclusive() - 1)
// }
//
// fn code_range_inclusive() -> RangeInclusive<usize> {
//     // Notice the subtraction to turn the exclusive end into an inclusive end.
//     #[allow(clippy::range_minus_one)]
//     RangeInclusive::new(super::code_start(), super::code_end_exclusive() - 1)
// }
//
// fn remapped_mmio_range_inclusive() -> RangeInclusive<usize> {
//     // The last 64 KiB slot in the first 512 MiB
//     RangeInclusive::new(0x1FFF_0000, 0x1FFF_FFFF)
// }
//
// fn mmio_range_inclusive() -> RangeInclusive<usize> {
//     RangeInclusive::new(memory_map::mmio::MMIO_BASE, memory_map::mmio::MMIO_END)
//     // RangeInclusive::new(map::phys::VIDEOMEM_BASE, map::mmio::MMIO_END),
// }
//
// fn dma_range_inclusive() -> RangeInclusive<usize> {
//     RangeInclusive::new(
//         memory_map::virt::DMA_HEAP_START,
//         memory_map::virt::DMA_HEAP_END,
//     )
// }
