// init_thread/src/el_switch.rs - EL2 to EL1 transition with VBAR setup

use {
    crate::loader::memory_barrier,
    aarch64_cpu::{
        asm,
        registers::{
            ELR_EL2, HCR_EL2, MAIR_EL1, ReadWriteable, SCTLR_EL1, SP_EL1, SPSR_EL1, SPSR_EL2,
            TCR_EL1, TTBR0_EL1, TTBR1_EL1, VBAR_EL1, Writeable,
        },
    },
};

/// Configure and enable the MMU, set VBAR_EL1, then drop to EL1
///
/// # Arguments
///
/// * `ttbr0` - Translation table base for low addresses (identity map)
/// * `ttbr1` - Translation table base for high addresses (kernel)
/// * `vbar` - Exception vector base address (physical, must be 2KB aligned)
/// * `entry_point` - Kernel entry point (virtual address)
/// * `stack_pointer` - Initial stack pointer for EL1
///
/// # Safety
///
/// This function never returns to the caller.
#[inline(never)]
pub unsafe fn enable_mmu_and_drop_to_el1(
    ttbr0: u64,
    ttbr1: u64,
    vbar: u64,
    entry_point: u64,
    stack_pointer: u64,
) -> ! {
    // MAIR: Memory Attribute Indirection Register
    // Index 0: Normal memory, Write-Back, Read/Write Allocate
    // Index 1: Device-nGnRnE memory
    let mair: u64 = 0xFF | (0x00 << 8);

    // ═══════════════════════════════════════════════════════════
    // STEP 1: Configure EL2 to allow EL1 operation
    // ═══════════════════════════════════════════════════════════

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);
    // @todo disable VM bit to prevent stage 2 MMU translations

    // ═══════════════════════════════════════════════════════════
    // STEP 2: Set up VBAR_EL1 (Exception Vector Base Address)
    // ═══════════════════════════════════════════════════════════

    // The address must be 2KB aligned (bits [10:0] must be 0).
    // We set the virtual address here since VBAR_EL1 is only
    // used after MMU is enabled (exceptions before ERET would
    // be taken at EL2, not EL1).
    VBAR_EL1.set(vbar);

    // ═══════════════════════════════════════════════════════════
    // STEP 3: Configure EL1 MMU settings
    // ═══════════════════════════════════════════════════════════

    MAIR_EL1.set(mair);

    TCR_EL1.write(
        TCR_EL1::TBI0::Ignored // Top byte ignored, can be used for tagging.
            // + TCR_EL1::IPS.val(ips) // Intermediate Physical Address Size
            // ttbr0 user memory addresses
            + TCR_EL1::TG0::KiB_4 // 4 KiB granule
            + TCR_EL1::SH0::Inner
            + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::EPD0::EnableTTBR0Walks
            + TCR_EL1::T0SZ.val(16) // T0SZ = 16 (48-bit VA for TTBR0)
            // ttbr1 kernel memory addresses
            // + TCR_EL1::TBI1::Ignored // Top byte ignored, can be used for tagging. @todo remove!
            + TCR_EL1::TG1::KiB_4 // 4 KiB granule
            + TCR_EL1::SH1::Inner
            + TCR_EL1::ORGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN1::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            // + TCR_EL1::EPD1::DisableTTBR1Walks // @fixme disabled for now
            + TCR_EL1::T1SZ.val(16), // T1SZ = 16 (48-bit VA for TTBR1)
    );

    TTBR0_EL1.set(ttbr0);
    TTBR1_EL1.set(ttbr1);

    // ═══════════════════════════════════════════════════════════
    // STEP 4: Prepare to drop to EL1 with MMU enabled
    // ═══════════════════════════════════════════════════════════

    // Enable MMU (takes effect after ERET)
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
            + SCTLR_EL1::C::Cacheable
            + SCTLR_EL1::I::Cacheable
            + SCTLR_EL1::M::Enable,
    );

    // SPSR_EL2: EL1h with DAIF masked
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1h, // Use SP_EL1, Return to EL1
    );

    // TODO: Mark interrupts in EL1
    SPSR_EL1.write(
        SPSR_EL1::D::Masked
            + SPSR_EL1::A::Masked
            + SPSR_EL1::I::Masked
            + SPSR_EL1::F::Masked
            + SPSR_EL1::M::EL1h, // Use SP_EL1
    );

    // Set return address and stack
    ELR_EL2.set(entry_point);
    SP_EL1.set(stack_pointer);

    memory_barrier();

    // ═══════════════════════════════════════════════════════════
    // STEP 5: Drop to EL1
    // ═══════════════════════════════════════════════════════════
    asm::eret()
}

/// Invalidate all TLB entries
#[inline(always)]
pub fn tlb_invalidate_all() {
    unsafe {
        core::arch::asm!(
            "dsb ishst",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            options(nostack, preserves_flags)
        );
    }
}
