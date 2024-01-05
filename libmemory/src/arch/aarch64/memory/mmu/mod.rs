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

use {
    crate::{
        memory::{
            mmu::{interface, interface::MMU, AddressSpace, MMUEnableError, TranslationGranule},
            Address, Physical,
        },
        platform, println,
    },
    aarch64_cpu::{
        asm::barrier,
        registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1, TTBR0_EL1},
    },
    core::intrinsics::unlikely,
    tock_registers::interfaces::{ReadWriteable, Readable, Writeable},
};

pub mod translation_table;

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

/// Memory Management Unit type.
struct MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

pub type Granule512MiB = TranslationGranule<{ 512 * 1024 * 1024 }>;
pub type Granule64KiB = TranslationGranule<{ 64 * 1024 }>;

/// Constants for indexing the MAIR_EL1.
#[allow(dead_code)]
pub mod mair {
    // Three descriptive consts for indexing into the correct MAIR_EL1 attributes.
    pub mod attr {
        pub const NORMAL: u64 = 0;
        pub const NORMAL_NON_CACHEABLE: u64 = 1;
        pub const DEVICE_NGNRE: u64 = 2;
    }
}

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static MMU: MemoryManagementUnit = MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Private Implementations
//--------------------------------------------------------------------------------------------------

impl<const AS_SIZE: usize> AddressSpace<AS_SIZE> {
    /// Checks for architectural restrictions.
    pub const fn arch_address_space_size_sanity_checks() {
        // Size must be at least one full 512 MiB table.
        assert!((AS_SIZE % Granule512MiB::SIZE) == 0); // assert!() is const-friendly

        // Check for 48 bit virtual address size as maximum, which is supported by any ARMv8
        // version.
        assert!(AS_SIZE <= (1 << 48));
    }
}

impl MemoryManagementUnit {
    /// Setup function for the MAIR_EL1 register.
    fn set_up_mair(&self) {
        use aarch64_cpu::registers::MAIR_EL1;
        // Define the three memory types that we will map: Normal DRAM, Uncached and device.
        MAIR_EL1.write(
            // Attribute 2 -- Device Memory
            MAIR_EL1::Attr2_Device::nonGathering_nonReordering_EarlyWriteAck
                // Attribute 1 -- Non Cacheable DRAM
                + MAIR_EL1::Attr1_Normal_Outer::NonCacheable
                + MAIR_EL1::Attr1_Normal_Inner::NonCacheable
                // Attribute 0 -- Regular Cacheable
                + MAIR_EL1::Attr0_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc
                + MAIR_EL1::Attr0_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc,
        );
    }

    /// Configure various settings of stage 1 of the EL1 translation regime.
    fn configure_translation_control(&self) {
        let t0sz = (64 - platform::memory::mmu::KernelVirtAddrSpace::SIZE_SHIFT) as u64;

        TCR_EL1.write(
            TCR_EL1::TBI0::Used
                + TCR_EL1::IPS::Bits_40
                + TCR_EL1::TG0::KiB_64
                + TCR_EL1::SH0::Inner
                + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD0::EnableTTBR0Walks
                + TCR_EL1::A1::TTBR0 // TTBR0 defines the ASID
                + TCR_EL1::T0SZ.val(t0sz)
                + TCR_EL1::EPD1::DisableTTBR1Walks,
        );
    }
}

//--------------------------------------------------------------------------------------------------
// Public Implementations
//--------------------------------------------------------------------------------------------------

/// Return a reference to the MMU instance.
pub fn mmu() -> &'static impl interface::MMU {
    &MMU
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl interface::MMU for MemoryManagementUnit {
    unsafe fn enable_mmu_and_caching(
        &self,
        phys_tables_base_addr: Address<Physical>,
    ) -> Result<(), MMUEnableError> {
        if unlikely(self.is_enabled()) {
            return Err(MMUEnableError::AlreadyEnabled);
        }

        // Fail early if translation granule is not supported.
        if unlikely(!ID_AA64MMFR0_EL1.matches_all(ID_AA64MMFR0_EL1::TGran64::Supported)) {
            return Err(MMUEnableError::Other {
                err: "Translation granule not supported by hardware",
            });
        }

        // Prepare the memory attribute indirection register.
        self.set_up_mair();

        // // Populate translation tables.
        // KERNEL_TABLES
        //     .populate_translation_table_entries()
        //     .map_err(|err| MMUEnableError::Other { err })?;

        // Set the "Translation Table Base Register".
        TTBR0_EL1.set_baddr(phys_tables_base_addr.as_usize() as u64);

        self.configure_translation_control();

        // Switch the MMU on.
        //
        // First, force all previous changes to be seen before the MMU is enabled.
        barrier::isb(barrier::SY);

        // Enable the MMU and turn on data and instruction caching.
        SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);

        // Force MMU init to complete before next instruction.
        barrier::isb(barrier::SY);

        Ok(())
    }

    #[inline(always)]
    fn is_enabled(&self) -> bool {
        SCTLR_EL1.matches_all(SCTLR_EL1::M::Enable)
    }

    /// Parse the ID_AA64MMFR0_EL1 register for runtime information about supported MMU features.
    /// Print the current state of TCR register.
    fn print_features(&self) {
        // use crate::cortex_a::regs::RegisterReadWrite;
        let sctlr = SCTLR_EL1.extract();

        if let Some(SCTLR_EL1::M::Value::Enable) = sctlr.read_as_enum(SCTLR_EL1::M) {
            println!("[i] MMU currently enabled");
        }

        if let Some(SCTLR_EL1::I::Value::Cacheable) = sctlr.read_as_enum(SCTLR_EL1::I) {
            println!("[i] MMU I-cache enabled");
        }

        if let Some(SCTLR_EL1::C::Value::Cacheable) = sctlr.read_as_enum(SCTLR_EL1::C) {
            println!("[i] MMU D-cache enabled");
        }

        let mmfr = ID_AA64MMFR0_EL1.extract();

        if let Some(ID_AA64MMFR0_EL1::TGran4::Value::Supported) =
            mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran4)
        {
            println!("[i] MMU: 4 KiB granule supported!");
        }

        if let Some(ID_AA64MMFR0_EL1::TGran16::Value::Supported) =
            mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran16)
        {
            println!("[i] MMU: 16 KiB granule supported!");
        }

        if let Some(ID_AA64MMFR0_EL1::TGran64::Value::Supported) =
            mmfr.read_as_enum(ID_AA64MMFR0_EL1::TGran64)
        {
            println!("[i] MMU: 64 KiB granule supported!");
        }

        match mmfr.read_as_enum(ID_AA64MMFR0_EL1::ASIDBits) {
            Some(ID_AA64MMFR0_EL1::ASIDBits::Value::Bits_16) => {
                println!("[i] MMU: 16 bit ASIDs supported!")
            }
            Some(ID_AA64MMFR0_EL1::ASIDBits::Value::Bits_8) => {
                println!("[i] MMU: 8 bit ASIDs supported!")
            }
            _ => println!("[i] MMU: Invalid ASID bits specified!"),
        }

        match mmfr.read_as_enum(ID_AA64MMFR0_EL1::PARange) {
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_32) => {
                println!("[i] MMU: Up to 32 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_36) => {
                println!("[i] MMU: Up to 36 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_40) => {
                println!("[i] MMU: Up to 40 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_42) => {
                println!("[i] MMU: Up to 42 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_44) => {
                println!("[i] MMU: Up to 44 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_48) => {
                println!("[i] MMU: Up to 48 Bit physical address range supported!")
            }
            Some(ID_AA64MMFR0_EL1::PARange::Value::Bits_52) => {
                println!("[i] MMU: Up to 52 Bit physical address range supported!")
            }
            _ => println!("[i] MMU: Invalid PARange specified!"),
        }

        let tcr = TCR_EL1.extract();

        match tcr.read_as_enum(TCR_EL1::IPS) {
            Some(TCR_EL1::IPS::Value::Bits_32) => {
                println!("[i] MMU: 32 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_36) => {
                println!("[i] MMU: 36 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_40) => {
                println!("[i] MMU: 40 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_42) => {
                println!("[i] MMU: 42 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_44) => {
                println!("[i] MMU: 44 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_48) => {
                println!("[i] MMU: 48 Bit intermediate physical address size supported!")
            }
            Some(TCR_EL1::IPS::Value::Bits_52) => {
                println!("[i] MMU: 52 Bit intermediate physical address size supported!")
            }
            _ => println!("[i] MMU: Invalid IPS specified!"),
        }

        match tcr.read_as_enum(TCR_EL1::TG0) {
            Some(TCR_EL1::TG0::Value::KiB_4) => println!("[i] MMU: TTBR0 4 KiB granule active!"),
            Some(TCR_EL1::TG0::Value::KiB_16) => println!("[i] MMU: TTBR0 16 KiB granule active!"),
            Some(TCR_EL1::TG0::Value::KiB_64) => println!("[i] MMU: TTBR0 64 KiB granule active!"),
            _ => println!("[i] MMU: Invalid TTBR0 granule size specified!"),
        }

        let t0sz = tcr.read(TCR_EL1::T0SZ);
        println!("[i] MMU: T0sz = 64-{} = {} bits", t0sz, 64 - t0sz);

        match tcr.read_as_enum(TCR_EL1::TG1) {
            Some(TCR_EL1::TG1::Value::KiB_4) => println!("[i] MMU: TTBR1 4 KiB granule active!"),
            Some(TCR_EL1::TG1::Value::KiB_16) => println!("[i] MMU: TTBR1 16 KiB granule active!"),
            Some(TCR_EL1::TG1::Value::KiB_64) => println!("[i] MMU: TTBR1 64 KiB granule active!"),
            _ => println!("[i] MMU: Invalid TTBR1 granule size specified!"),
        }

        let t1sz = tcr.read(TCR_EL1::T1SZ);
        println!("[i] MMU: T1sz = 64-{} = {} bits", t1sz, 64 - t1sz);
    }
}
