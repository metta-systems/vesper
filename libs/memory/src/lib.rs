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
    libaddress::{Address, Physical},
    snafu::Snafu,
    translation_table::interface::TranslationTable,
};

pub use crate::arch::mmu as arch_mmu;

mod arch;
pub mod page_alloc;
pub mod translation_table;

//--------------------------------------------------------------------------------------------------
// Architectural Public Reexports
//--------------------------------------------------------------------------------------------------
// pub use arch_mmu::*;

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
        // Self::arch_address_space_size_sanity_checks();

        AS_SIZE
    }
}
