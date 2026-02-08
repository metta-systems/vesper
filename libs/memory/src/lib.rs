/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! The arch-independent representation of a MMU and memory translation tables.

// To test we need to impl this for x86_64, aarch64 and riscv64 arches
// and provide the same interface from the arch-independent layer.

// Need to be able to
// a) create page table hierarchy at different granule size and addressing mode (user/kernel)
// b) inspect/walk table hierarchy and resolve virtual-to-physical addresses via provided tables
// c) modify/invalidate page hierarchy descriptors

#![no_std]
#![allow(dead_code)] // while refactoring
#![allow(incomplete_features)]
#![feature(generic_const_exprs)] // incomplete_features
#![feature(const_trait_impl)]
#![feature(format_args_nl)]
#![allow(internal_features)]
#![feature(allocator_api)]
#![feature(core_intrinsics)]
#![feature(step_trait)]
#![feature(custom_test_frameworks)]

use {
    core::num::NonZeroUsize,
    interface::MMU,
    libaddress::{Address, Physical, Virtual},
    liblocking::interface::*,
    liblog::warn,
    snafu::Snafu,
    translation_table::interface::TranslationTable,
};

#[cfg(target_arch = "aarch64")]
use crate::arch::aarch64::mmu as arch_mmu;
use crate::platform::memory::KernelGranule;

mod arch;
mod mapping_record;
pub mod page_alloc;
pub mod translation_table;
mod types;

pub use types::*;

/// Convert a size into human readable format.
pub const fn size_human_readable_ceil(size: usize) -> (usize, &'static str) {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * 1024;
    const GIB: usize = 1024 * 1024 * 1024;

    if (size / GIB) > 0 {
        (size.div_ceil(GIB), "GiB")
    } else if (size / MIB) > 0 {
        (size.div_ceil(MIB), "MiB")
    } else if (size / KIB) > 0 {
        (size.div_ceil(KIB), "KiB")
    } else {
        (size, "Byte")
    }
}

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
// pub use arch_mmu::mmu;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// MMU enable errors variants.
// @todo rework error types
#[allow(missing_docs)]
#[derive(Debug, Snafu)]
pub enum MMUEnableError {
    #[snafu(display("MMU is already enabled"))]
    AlreadyEnabled,
    #[snafu(display("{}", err))]
    Other { err: &'static str },
}

/// Memory Management interfaces.
pub mod interface {
    use super::*;

    /// MMU functions.
    pub trait MMU {
        /// Turns on the MMU for the first time and enables data and instruction caching.
        ///
        /// # Safety
        ///
        /// - Changes the hardware's global state.
        unsafe fn enable_mmu_and_caching(
            &self,
            phys_tables_base_addr: Address<Physical>,
        ) -> Result<(), MMUEnableError>;

        /// Returns true if the MMU is enabled, false otherwise.
        fn is_enabled(&self) -> bool;

        fn print_features(&self); // debug
    }
}

/// Describes the characteristics of a translation granule.
pub struct TranslationGranule<const GRANULE_SIZE: usize>;

/// Describes properties of an address space.
pub struct AddressSpace<const AS_SIZE: usize>;

/// Intended to be implemented for [`AddressSpace`].
pub trait AssociatedTranslationTable {
    /// A translation table whose address range is:
    ///
    /// [`AS_SIZE` - 1, 0]
    type TableStartFromBottom;
}

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

// Query the platform for the reserved virtual addresses for MMIO remapping
// and initialize the kernel's MMIO VA allocator with it.
// fn kernel_init_mmio_va_allocator() {
//     let region = platform::memory::mmu::virt_mmio_remap_region();
//     page_alloc::kernel_mmio_va_allocator().lock(|allocator| allocator.init(region));
// }

/// Map a region in the kernel's translation tables.
///
/// No input checks done, input is passed through to the architectural implementation.
///
/// # Safety
///
/// - See `map_at()`.
/// - Does not prevent aliasing.
unsafe fn kernel_map_at_unchecked(
    name: &'static str,
    virt_region: &MemoryRegion<Virtual>,
    phys_region: &MemoryRegion<Physical>,
    attr: AttributeFields,
) -> Result<(), &'static str> {
    crate::platform::memory::mmu::kernel_translation_tables().write(|tables|
            // SAFETY: Make a mistake and you're dead, gaijin!
            unsafe { tables.map_at(virt_region, phys_region, attr) })?;

    if let Err(x) = mapping_record::kernel_add(name, virt_region, phys_region, attr) {
        warn!("{x}");
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl<const GRANULE_SIZE: usize> TranslationGranule<GRANULE_SIZE> {
    /// The granule's size.
    pub const SIZE: usize = Self::size_checked();

    /// The granule's mask.
    pub const MASK: usize = Self::SIZE - 1;

    /// The granule's shift, aka log2(size).
    pub const SHIFT: usize = Self::SIZE.trailing_zeros() as usize;

    const fn size_checked() -> usize {
        assert!(GRANULE_SIZE.is_power_of_two());

        GRANULE_SIZE
    }
}

impl<const AS_SIZE: usize> AddressSpace<AS_SIZE> {
    /// The address space size.
    pub const SIZE: usize = Self::size_checked();

    /// The address space shift, aka log2(size).
    pub const SIZE_SHIFT: usize = Self::SIZE.trailing_zeros() as usize;

    const fn size_checked() -> usize {
        assert!(AS_SIZE.is_power_of_two());

        // Check for architectural restrictions as well.
        Self::arch_address_space_size_sanity_checks();

        AS_SIZE
    }
}

//--------------------------------------------------------------------------------------------------
// Public Code
// FIXME: this code should move to init_thread and be only used during boot-up! (see paging.rs)
//--------------------------------------------------------------------------------------------------

/// Raw mapping of a virtual to physical region in the kernel translation tables.
///
/// Prevents mapping into the MMIO range of the tables.
///
/// # Safety
///
/// - See `kernel_map_at_unchecked()`.
/// - Does not prevent aliasing. Currently, the callers must be trusted.
pub unsafe fn kernel_map_at(
    name: &'static str,
    virt_region: &MemoryRegion<Virtual>,
    phys_region: &MemoryRegion<Physical>,
    attr: AttributeFields,
) -> Result<(), &'static str> {
    if platform::memory::mmu::virt_mmio_remap_region().overlaps(virt_region) {
        return Err("Attempt to manually map into MMIO region");
    }

    // SAFETY: Make a mistake and you're dead, gaijin!
    unsafe {
        kernel_map_at_unchecked(name, virt_region, phys_region, attr)?;
    }

    Ok(())
}

/// MMIO remapping in the kernel translation tables.
///
/// Typically used by device drivers.
///
/// # Safety
///
/// - Same as `kernel_map_at_unchecked()`, minus the aliasing part.
pub unsafe fn kernel_map_mmio(
    name: &'static str,
    mmio_descriptor: &MMIODescriptor,
) -> Result<Address<Virtual>, &'static str> {
    let phys_region = MemoryRegion::from(*mmio_descriptor);
    let offset_into_start_page = mmio_descriptor
        .start_addr()
        .offset_into_page(&KernelGranule::SIZE); // FIXME: fixed page size

    // Check if an identical region has been mapped for another driver. If so, reuse it.
    let virt_addr = if let Some(addr) =
        mapping_record::kernel_find_and_insert_mmio_duplicate(mmio_descriptor, name)
    {
        addr
        // Otherwise, allocate a new region and map it.
    } else {
        let Some(num_pages) = NonZeroUsize::new(phys_region.num_pages()) else {
            return Err("Requested 0 pages");
        };

        let virt_region =
            page_alloc::kernel_mmio_va_allocator().lock(|allocator| allocator.alloc(num_pages))?;

        // SAFETY: Make a mistake and you're dead, gaijin!
        unsafe {
            kernel_map_at_unchecked(
                name,
                &virt_region,
                &phys_region,
                AttributeFields {
                    mem_attributes: MemAttributes::Device,
                    acc_perms: AccessPermissions::ReadWrite,
                    execute_never: true,
                },
            )?;
        }

        virt_region.start_addr()
    };

    Ok(virt_addr + offset_into_start_page)
}

/// Map the kernel's binary. Returns the translation table's base address.
///
/// # Safety
///
/// - See [`bsp::memory::mmu::kernel_map_binary()`].
pub unsafe fn kernel_map_binary() -> Result<Address<Physical>, &'static str> {
    let phys_kernel_tables_base_addr =
        platform::memory::mmu::kernel_translation_tables().write(|tables| {
            tables.init().unwrap();
            tables.phys_base_address()
        });

    // SAFETY: Make a mistake and you're dead, gaijin!
    unsafe {
        platform::memory::mmu::kernel_map_binary()?;
    }

    Ok(phys_kernel_tables_base_addr)
}

/// Enable the MMU and data + instruction caching.
///
/// # Safety
///
/// - Crucial function during kernel init. Changes the the complete memory view of the processor.
#[inline]
pub unsafe fn enable_mmu_and_caching(
    phys_tables_base_addr: Address<Physical>,
) -> Result<(), MMUEnableError> {
    // SAFETY: Make a mistake and you're dead, gaijin!
    unsafe { arch_mmu::mmu().enable_mmu_and_caching(phys_tables_base_addr) }
}

/// Human-readable print of all recorded kernel mappings.
#[inline]
pub fn kernel_print_mappings() {
    mapping_record::kernel_print();
}
