/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! AArch64 MMU hardware configuration and enablement.
//!
//! This module handles the actual hardware register setup (MAIR, TCR, SCTLR)
//! and MMU enable sequence. It does NOT manage translation table contents —
//! that is handled by the `Table` type and `Aarch64_4K` arch implementation.

use {
    aarch64_cpu::{
        asm::{self, barrier},
        registers::{ID_AA64MMFR0_EL1, SCTLR_EL1, TCR_EL1},
    },
    core::intrinsics::unlikely,
    libaddress::{Address, Physical},
    liblog::println,
    tock_registers::interfaces::{ReadWriteable, Readable, Writeable},
};

/// MMU enable errors.
#[derive(Debug)]
pub enum MMUEnableError {
    /// MMU is already enabled.
    AlreadyEnabled,
    /// Other error.
    Other { err: &'static str },
}

impl core::fmt::Display for MMUEnableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AlreadyEnabled => write!(f, "MMU is already enabled"),
            Self::Other { err } => write!(f, "{err}"),
        }
    }
}

/// Memory Management Unit type.
pub struct MemoryManagementUnit;

/// MAIR_EL1 attribute indices.
///
/// These must match the MAIR setup in `set_up_mair()` and the indices
/// used by `arch::aarch64::translation::encode_attributes()`.
pub mod mair {
    pub mod attr {
        pub const NORMAL: u64 = 0;
        pub const NORMAL_NON_CACHEABLE: u64 = 1;
        pub const DEVICE_NGNRE: u64 = 2;
    }
}

static MMU: MemoryManagementUnit = MemoryManagementUnit;

impl MemoryManagementUnit {
    /// Setup function for the MAIR_EL1 register.
    fn set_up_mair(&self) {
        use aarch64_cpu::registers::MAIR_EL1;
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
        let ips = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);

        // Maximum 8Gb user VA
        let user_va_bits = 33; // ARMv8ARM Table D5-11 minimum TxSZ for starting table level 1

        // Maximum 8Gb kernel VA
        let kernel_va_bits = 33;

        TCR_EL1.write(
            TCR_EL1::TBI0::Ignored
                + TCR_EL1::IPS.val(ips)
                // ttbr0 user memory addresses
                + TCR_EL1::TG0::KiB_4
                + TCR_EL1::SH0::Inner
                + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD0::EnableTTBR0Walks
                + TCR_EL1::T0SZ.val(64 - user_va_bits)
                // ttbr1 kernel memory addresses
                + TCR_EL1::TBI1::Ignored
                + TCR_EL1::TG1::KiB_4
                + TCR_EL1::SH1::Inner
                + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
                + TCR_EL1::EPD1::DisableTTBR1Walks // disabled for now
                + TCR_EL1::T1SZ.val(64 - kernel_va_bits),
        );
    }

    fn is_enabled(&self) -> bool {
        SCTLR_EL1.matches_all(SCTLR_EL1::M::Enable)
    }
}

/// Return a reference to the MMU instance.
pub fn mmu() -> &'static MemoryManagementUnit {
    &MMU
}

impl MemoryManagementUnit {
    /// Turns on the MMU and enables data and instruction caching.
    ///
    /// `phys_tables_base_addr` is the physical address of the root translation
    /// table (L0 or L1 depending on T0SZ configuration).
    ///
    /// # Safety
    ///
    /// - Changes the hardware's global state.
    /// - The caller must ensure translation tables are properly populated.
    pub unsafe fn enable_mmu_and_caching(
        &self,
        _phys_tables_base_addr: Address<Physical>,
    ) -> Result<(), MMUEnableError> {
        if unlikely(self.is_enabled()) {
            return Err(MMUEnableError::AlreadyEnabled);
        }

        // Fail early if translation granule is not supported.
        if unlikely(!ID_AA64MMFR0_EL1.matches_all(ID_AA64MMFR0_EL1::TGran4::Supported)) {
            return Err(MMUEnableError::Other {
                err: "4KiB translation granule not supported by hardware",
            });
        }

        // Prepare the memory attribute indirection register.
        self.set_up_mair();

        // TODO: Set TTBR0_EL1 and TTBR1_EL1 to point to the root table.
        // TTBR0_EL1.set_baddr(phys_tables_base_addr.as_u64());
        // TTBR0_EL1.modify(TTBR0_EL1::CnP.val(1));

        self.configure_translation_control();

        // Switch the MMU on.
        // First, force all previous changes to be seen before the MMU is enabled.
        barrier::dsb(barrier::ISHST);
        barrier::dsb(barrier::ISH);
        barrier::isb(barrier::SY);

        // Enable the MMU and turn on data and instruction caching.
        SCTLR_EL1.modify(
            SCTLR_EL1::EE::LittleEndian
                + SCTLR_EL1::E0E::LittleEndian
                + SCTLR_EL1::WXN::Disable
                + SCTLR_EL1::SA::Disable
                + SCTLR_EL1::SA0::Disable
                + SCTLR_EL1::A::Disable
                + SCTLR_EL1::UCI::Trap
                + SCTLR_EL1::UCT::Trap
                + SCTLR_EL1::UMA::Trap
                + SCTLR_EL1::NTWE::Trap
                + SCTLR_EL1::NTWI::Trap
                + SCTLR_EL1::DZE::Trap
                + SCTLR_EL1::C::Cacheable
                + SCTLR_EL1::I::Cacheable
                + SCTLR_EL1::M::Enable,
        );

        // Let 2 CPU cycles pass then invalidate TLB.
        asm::nop();
        asm::nop();

        barrier::dsb(barrier::ISH);
        barrier::isb(barrier::SY);

        println!("MMU activated");

        Ok(())
    }
}
