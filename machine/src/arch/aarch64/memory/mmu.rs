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
        memory::mmu::{
            interface, interface::MMU, translation_table::KernelTranslationTable, AddressSpace,
            MMUEnableError, TranslationGranule,
        },
        platform, println,
    },
    core::intrinsics::unlikely,
    cortex_a::{
        asm,
        asm::barrier,
        registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1, TTBR0_EL1, TTBR1_EL1},
    },
    tock_registers::interfaces::{ReadWriteable, Readable, Writeable},
};

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

/// The kernel translation tables.
///
/// # Safety
///
/// - Supposed to land in `.bss`. Therefore, ensure that all initial member values boil down to "0".
static mut KERNEL_TABLES: KernelTranslationTable = KernelTranslationTable::new();

static MMU: MemoryManagementUnit = MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Private Implementations
//--------------------------------------------------------------------------------------------------

impl<const AS_SIZE: usize> AddressSpace<AS_SIZE> {
    /// Checks for architectural restrictions.
    pub const fn arch_address_space_size_sanity_checks() {
        // Size must be at least one full 512 MiB table.
        assert!((AS_SIZE % Granule512MiB::SIZE) == 0);

        // Check for 48 bit virtual address size as maximum, which is supported by any ARMv8
        // version.
        assert!(AS_SIZE <= (1 << 48));
    }
}

impl MemoryManagementUnit {
    /// Setup function for the MAIR_EL1 register.
    fn set_up_mair(&self) {
        use cortex_a::registers::MAIR_EL1;
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
        // Configure various settings of stage 1 of the EL1 translation regime.
        let ips = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);
        TCR_EL1.write(
            TCR_EL1::TBI0::Ignored // Top byte ignored
                + TCR_EL1::IPS.val(ips) // Intermediate Physical Address Size
                // ttbr0 user memory addresses
                + TCR_EL1::TG0::KiB_4 // 4 KiB granule
                + TCR_EL1::SH0::Inner
                + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD0::EnableTTBR0Walks
                + TCR_EL1::T0SZ.val(34) // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
                // ttbr1 kernel memory addresses
                + TCR_EL1::TBI1::Ignored
                + TCR_EL1::TG1::KiB_4 // 4 KiB granule
                + TCR_EL1::SH1::Inner
                + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD1::EnableTTBR1Walks
                + TCR_EL1::T1SZ.val(34), // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 2
        );
    }
}

//--------------------------------------------------------------------------------------------------
// Public Implementations
//--------------------------------------------------------------------------------------------------

/// Return a reference to the MMU instance.
pub fn mmu() -> &'static impl MMU {
    &MMU
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl interface::MMU for MemoryManagementUnit {
    unsafe fn enable_mmu_and_caching(&self) -> Result<(), MMUEnableError> {
        if unlikely(self.is_enabled()) {
            return Err(MMUEnableError::AlreadyEnabled);
        }

        // Fail early if translation granule is not supported.
        if unlikely(!ID_AA64MMFR0_EL1.matches_all(ID_AA64MMFR0_EL1::TGran64::Supported)) {
            return Err(MMUEnableError::Other(
                "Translation granule not supported in HW",
            ));
        }

        // Prepare the memory attribute indirection register.
        self.set_up_mair();

        // Populate translation tables.
        KERNEL_TABLES
            .populate_translation_table_entries()
            .map_err(MMUEnableError::Other)?;

        // from https://lore.kernel.org/all/db9612a7-9354-2357-9083-1d923b4d11e1@linaro.org/T/
        // The ARMv8.2-TTCNP extension allows an implementation to optimize by
        // sharing TLB entries between multiple cores, provided that software
        // declares that it's ready to deal with this by setting a CnP bit in
        // the TTBRn_ELx.  It is mandatory from ARMv8.2 onward.

        // support feature flag is in ID_AA64MMFR2
        // https://developer.arm.com/documentation/ddi0601/2022-03/AArch64-Registers/ID-AA64MMFR2-EL1--AArch64-Memory-Model-Feature-Register-2?lang=en
        // CnP bits 3:0
        // From Armv8.2, the only permitted value is 0b0001.
        // (this should be set to share the TLBs across cores.)

        // Point to the LVL2 table base address in TTBR0.
        TTBR0_EL1.set_baddr(LVL2_TABLE.entries.base_addr_u64()); // User (lo-)space addresses
        TTBR1_EL1.set_baddr(LVL2_TABLE.entries.base_addr_u64()); // Kernel (hi-)space addresses

        // lower half, user space
        // asm volatile ("msr ttbr0_el1, %0" : : "r" ((unsigned long)&_end + TTBR_CNP));
        // upper half, kernel space
        // asm volatile ("msr ttbr1_el1, %0" : : "r" ((unsigned long)&_end + TTBR_CNP + PAGESIZE));

        self.configure_translation_control();

        // Switch the MMU on.
        //
        // First, force all previous changes to be seen before the MMU is enabled.
        barrier::dsb(barrier::ISH); // dsb ishst?
        barrier::isb(barrier::SY);

        // use cortex_a::regs::RegisterReadWrite;
        // Enable the MMU and turn on data and instruction caching.
        SCTLR_EL1.modify(
            SCTLR_EL1::EE::LittleEndian // Endianness select in EL1
                + SCTLR_EL1::E0E::LittleEndian // Endianness select in EL0
                + SCTLR_EL1::WXN::Disable // Writable means Execute Never
                + SCTLR_EL1::SA::Disable // SP Alignment check in EL1, 16 byte align
                + SCTLR_EL1::SA0::Disable // SP Alignment check in EL0, 16 byte align
                + SCTLR_EL1::A::Disable // No alignment checks
                + SCTLR_EL1::UCI::Trap // Unified Cache instructions trap
                + SCTLR_EL1::UCT::Trap // CTR_EL0 instructions trap
                + SCTLR_EL1::UMA::Trap // User Mask Access, trap on DAIF access
                + SCTLR_EL1::NTWE::Trap // WFE/WFET instruction trap
                + SCTLR_EL1::NTWI::Trap // WFI/WFIT instruction trap
                + SCTLR_EL1::DZE::Trap // DC ZVA/GVA/GZVA instructions trap
                + SCTLR_EL1::C::Cacheable // No caching at all
                + SCTLR_EL1::I::Cacheable // No instruction cache
                + SCTLR_EL1::M::Disable,
        );

        // from https://forums.raspberrypi.com/viewtopic.php?t=320120#p1917769
        // Another hint: once the MMU has been activated you should let 2 CPU cycles pass and then call
        // `tlbi alle2` to ensure the MMU related cache will be invalidated and the new settings are picked up.

        asm::nop();
        asm::nop();

        // Force MMU init to complete before next instruction
        /*
         * Invalidate the local I-cache so that any instructions fetched
         * speculatively from the PoC are discarded, since they may have
         * been dynamically patched at the PoU.
         */
        barrier::isb(barrier::SY);

        println!("MMU activated");

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
