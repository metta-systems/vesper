use {super::map as memory_map, crate::memory::mmu::*, core::ops::RangeInclusive};

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// The kernel's address space defined by this BSP.
pub type KernelAddrSpace = AddressSpace<{ memory_map::END_INCLUSIVE + 1 }>;

const NUM_MEM_RANGES: usize = 6;

/// The virtual memory layout that is agnostic of the paging granularity that the
/// hardware MMU will use.
///
/// Contains only special ranges, aka anything that is _not_ normal cacheable
/// DRAM.
pub static LAYOUT: KernelVirtualLayout<NUM_MEM_RANGES> = KernelVirtualLayout::new(
    memory_map::END_INCLUSIVE,
    [
        TranslationDescriptor {
            name: "Boot code and data",
            virtual_range: boot_range_inclusive,
            physical_range_translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::CacheableDRAM,
                acc_perms: AccessPermissions::ReadOnly,
                execute_never: false,
            },
        },
        TranslationDescriptor {
            name: "Kernel code and RO data",
            virtual_range: code_range_inclusive,
            physical_range_translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::CacheableDRAM,
                acc_perms: AccessPermissions::ReadOnly,
                execute_never: false,
            },
        },
        TranslationDescriptor {
            name: "Remapped Device MMIO",
            virtual_range: remapped_mmio_range_inclusive,
            physical_range_translation: Translation::Offset(
                memory_map::phys::MMIO_BASE + 0x20_0000,
            ),
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::Device,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            },
        },
        TranslationDescriptor {
            name: "Device MMIO",
            virtual_range: mmio_range_inclusive,
            physical_range_translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::Device,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            },
        },
        TranslationDescriptor {
            name: "DMA heap pool",
            virtual_range: dma_range_inclusive,
            physical_range_translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::NonCacheableDRAM,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            },
        },
        TranslationDescriptor {
            name: "Framebuffer area (static for now)",
            virtual_range: || {
                RangeInclusive::new(
                    memory_map::phys::VIDEOMEM_BASE,
                    memory_map::phys::MMIO_BASE - 1,
                )
            },
            physical_range_translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::Device,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            },
        },
    ],
);

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

fn boot_range_inclusive() -> RangeInclusive<usize> {
    RangeInclusive::new(super::boot_start(), super::boot_end_exclusive() - 1)
}

fn code_range_inclusive() -> RangeInclusive<usize> {
    // Notice the subtraction to turn the exclusive end into an inclusive end.
    #[allow(clippy::range_minus_one)]
    RangeInclusive::new(super::code_start(), super::code_end_exclusive() - 1)
}

fn remapped_mmio_range_inclusive() -> RangeInclusive<usize> {
    // The last 64 KiB slot in the first 512 MiB
    RangeInclusive::new(0x1FFF_0000, 0x1FFF_FFFF)
}

fn mmio_range_inclusive() -> RangeInclusive<usize> {
    RangeInclusive::new(memory_map::phys::MMIO_BASE, memory_map::phys::MMIO_END)
    // RangeInclusive::new(map::phys::VIDEOMEM_BASE, map::phys::MMIO_END),
}

fn dma_range_inclusive() -> RangeInclusive<usize> {
    RangeInclusive::new(
        memory_map::virt::DMA_HEAP_START,
        memory_map::virt::DMA_HEAP_END,
    )
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return a reference to the virtual memory layout.
pub fn virt_mem_layout() -> &'static KernelVirtualLayout<NUM_MEM_RANGES> {
    &LAYOUT
}
