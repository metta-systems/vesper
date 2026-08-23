#![no_std]
#![feature(const_trait_impl)]
#![feature(step_trait)]

use {
    libaddress::{Address, Virtual},
    libmmio::MMIODescriptor,
};

mod mapping_attributes;
mod mapping_record;
mod memory_region;
mod page_address;

pub use {mapping_attributes::*, memory_region::*, page_address::*};

//--------------------------------------------------------------------------------------------------
// Public Code
// FIXME: this code should move to kickstart and be only used during boot-up! (see paging.rs)
//--------------------------------------------------------------------------------------------------

// Raw mapping of a virtual to physical region in the kernel translation tables.
//
// Prevents mapping into the MMIO range of the tables.
//
// Safety
//
// - See `kernel_map_at_unchecked()`.
// - Does not prevent aliasing. Currently, the callers must be trusted.
// pub unsafe fn kernel_map_at(
//     name: &'static str,
//     virt_region: &MemoryRegion<Virtual>,
//     phys_region: &MemoryRegion<Physical>,
//     attr: AttributeFields,
// ) -> Result<(), &'static str> {
//     // if platform::memory::mmu::virt_mmio_remap_region().overlaps(virt_region) {
//     //     return Err("Attempt to manually map into MMIO region");
//     // }
//     unsafe {
//         kernel_map_at_unchecked(name, virt_region, phys_region, attr)?;
//     }
//     Ok(())
// }

/// MMIO remapping in the kernel translation tables.
///
/// Typically used by device drivers.
///
/// # Safety
///
/// - Same as `kernel_map_at_unchecked()`, minus the aliasing part.
pub unsafe fn kernel_map_mmio(
    _name: &'static str,
    mmio_descriptor: &MMIODescriptor,
) -> Result<Address<Virtual>, &'static str> {
    // let phys_region = MemoryRegion::from(*mmio_descriptor);
    // let offset_into_start_page = mmio_descriptor.start_addr().offset_into_page(&4096); // FIXME: hardcoded page size

    // // Check if an identical region has been mapped for another driver. If so, reuse it.
    // let virt_addr = if let Some(addr) =
    //     mapping_record::kernel_find_and_insert_mmio_duplicate(mmio_descriptor, name)
    // {
    //     addr
    //     // Otherwise, allocate a new region and map it.
    // } else {
    //     let Some(num_pages) = NonZeroUsize::new(phys_region.num_pages()) else {
    //         return Err("Requested 0 pages");
    //     };

    //     let virt_region =
    //         page_alloc::kernel_mmio_va_allocator().lock(|allocator| allocator.alloc(num_pages))?;

    //     unsafe {
    //         kernel_map_at_unchecked(
    //             name,
    //             &virt_region,
    //             &phys_region,
    //             AttributeFields {
    //                 mem_attributes: MemAttributes::Device,
    //                 acc_perms: AccessPermissions::ReadWrite,
    //                 execute_never: true,
    //             },
    //         )?;
    //     }

    //     virt_region.start_addr()
    // };

    // Ok(virt_addr + offset_into_start_page)
    Ok(mmio_descriptor.start_addr().as_usize().into())
}

// Map a region in the kernel's translation tables.
//
// No input checks done, input is passed through to the architectural implementation (syscall?).
//
// # Safety
//
// - See `map_at()`.
// - Does not prevent aliasing.
// unsafe fn kernel_map_at_unchecked(
//     name: &'static str,
//     virt_region: &MemoryRegion<Virtual, PAGE_SIZE>,
//     phys_region: &MemoryRegion<Physical>,
//     attr: AttributeFields,
// ) -> Result<(), &'static str> {
// crate::platform::memory::mmu::kernel_translation_tables().write(|tables|
//         // SAFETY: Make a mistake and you're dead, gaijin!
//         unsafe { tables.map_at(virt_region, phys_region, attr) })?;

// if let Err(x) = mapping_record::kernel_add(name, virt_region, phys_region, attr) {
//     // warn!("{x}");
//     return Err(x);
// }

//     Ok(())
// }
