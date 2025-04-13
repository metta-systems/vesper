/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

//! MMU initialisation.
//!
//! Paging is mostly based on [previous version](https://os.phil-opp.com/page-tables/) of
//! Phil Opp's [paging guide](https://os.phil-opp.com/paging-implementation/) and
//! [ARMv8 ARM memory addressing](https://static.docs.arm.com/100940/0100/armv8_a_address%20translation_100940_0100_en.pdf).

use crate::{
    memory::{
        Address, Physical, Virtual,
        mmu::{self as generic_mmu, PageSize, PageSizeSupport, PageTable, PageTableFlags},
    },
    platform,
    synchronization::{InitStateLock, interface::ReadWriteEx},
};

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

pub struct AArch64PageSizes;

impl PageSizeSupport for AArch64PageSizes {
    fn supported_sizes(&self) -> &[PageSize] {
        static SIZES: &[PageSize] = &[
            PageSize::new(64 * 1024, 0),       // 64KB
            PageSize::new(512 * 1024, 1),      // 512KB
            PageSize::new(1024 * 1024, 1),     // 1MB
            PageSize::new(2 * 1024 * 1024, 2), // 2MB
        ];
        SIZES
    }
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub type Granule512MiB = PageSize;
pub type Granule64KiB = PageSize;

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static PAGE_SIZES: AArch64PageSizes = AArch64PageSizes;

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl PageTable<AArch64PageSizes> {
    fn make_entry(
        &self,
        phys: Address<Physical>,
        page_size: PageSize,
        flags: PageTableFlags,
    ) -> PageTableEntry<AArch64PageSizes> {
        // AArch64-specific page table entry creation
        unimplemented!()
    }
}
