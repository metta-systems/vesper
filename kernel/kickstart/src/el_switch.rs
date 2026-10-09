// kickstart/src/el_switch.rs - EL2 to EL1 transition with VBAR setup

use {
    crate::loader::memory_barrier,
    aarch64_cpu::{
        asm::{self, barrier},
        registers::{
            CNTKCTL_EL1, CPACR_EL1, ELR_EL2, HCR_EL2, MAIR_EL1, MDSCR_EL1, PMUSERENR_EL0,
            ReadWriteable, SCTLR_EL1, SP_EL0, SP_EL1, SPSR_EL1, SPSR_EL2, TCR_EL1, TPIDR_EL0,
            TPIDRRO_EL0, TTBR0_EL1, TTBR1_EL1, VBAR_EL1, Writeable,
        },
    },
};

/// Establish the integer-only FP/SIMD policy before anything runs at EL1 or
/// EL0: architectural reset leaves this control UNKNOWN, so it is set
/// explicitly. Any FP/SIMD instruction (and, with SVE, any SVE instruction)
/// at EL1 or EL0 then traps to EL1 with `ESR_EL1.EC` 0x07 (0x19 for SVE) — an
/// execution fault, never an automatic enable. (`libboot`'s
/// `_startup_in_rust` has already cleared the `CPTR_EL2` traps, so these
/// traps are taken to EL1.)
fn configure_fp_simd_traps() {
    CPACR_EL1.write(CPACR_EL1::FPEN::TrapEl0El1 + CPACR_EL1::ZEN::TrapEl0El1);
    barrier::isb(barrier::SY);
}

/// Establish the selected EL0-visible architectural state (see the contract's
/// "Selected EL0-visible architectural state"): reset leaves all of it
/// UNKNOWN.
///
/// - TLS: `TPIDR_EL0` starts at 0 (from here on it is per-Thread state carried
///   by the exception frame); `TPIDRRO_EL0` is 0 and nothing else writes it.
///   Linux uses `TPIDRRO_EL0` to publish identity that EL0 may read but not
///   change — a possible future use, not selected.
/// - Generic timer: EL0 may read the virtual counter (and `CNTFRQ_EL0`) only;
///   the physical counter and every timer trap, and no event stream runs.
/// - Debug: EL0 access to the debug communications channel traps; software
///   debug (breakpoints, watchpoints, single step) stays disabled.
/// - Performance monitors: every EL0 access traps.
fn configure_el0_visible_state() {
    TPIDR_EL0.set(0);
    TPIDRRO_EL0.set(0);
    CNTKCTL_EL1.write(
        CNTKCTL_EL1::EL0PTEN::TrappedPhysical
            + CNTKCTL_EL1::EL0VTEN::TrappedVirtual
            + CNTKCTL_EL1::EVNTEN::Disable
            + CNTKCTL_EL1::EL0VCTEN::TrappedNone
            + CNTKCTL_EL1::EL0PCTEN::TrappedFreqPct,
    );
    MDSCR_EL1.write(MDSCR_EL1::TDCC::SET);
    PMUSERENR_EL0.write(
        PMUSERENR_EL0::ER::TrappedUnlessEnabled
            + PMUSERENR_EL0::CR::TrappedUnlessEnabled
            + PMUSERENR_EL0::SW::TrappedUnlessEnabled
            + PMUSERENR_EL0::EN::TrappedUnlessEnabled,
    );
    barrier::isb(barrier::SY);
}

/// Configure and enable the MMU, set `VBAR_EL1`, then drop to EL1
///
/// # Arguments
///
/// * `ttbr0` - Translation table base for low addresses (identity map)
/// * `ttbr1` - Translation table base for high addresses (kernel)
/// * `vbar` - Exception vector base address (physical, must be 2KB aligned)
/// * `entry_point` - Kernel entry point (virtual address)
/// * `execution_stack_pointer` - Initial execution stack pointer in `SP_EL0` (`EL1t`)
/// * `trap_stack_pointer` - Shared per-core kernel trap stack pointer in `SP_EL1`
///
/// Implementation status: the entry is trusted linked `Kickstart`/`Kicktest` code,
/// not a protected EL0 Thread; `vbar` is its mapped high virtual address.
///
/// # Safety
///
/// This function never returns to the caller.
/// The caller must run at EL2 with `SP_EL2` selected and only the boot core active.
/// The translation tables must remain live and map the entry executable, the
/// vectors executable, and both distinct stack extents writable. Both stack
/// pointers must be 16-byte aligned tops of sufficiently sized, retained backing.
/// No live continuation or borrowed stack storage may be overwritten by either
/// stack; reusing the EL2 boot stack for execution abandons the EL2 call chain.
/// The trap stack must stay reserved for this core's kernel exception handling.
#[inline(never)]
pub unsafe fn enable_mmu_and_drop_to_el1(
    ttbr0: u64,
    ttbr1: u64,
    vbar: u64,
    entry_point: u64,
    execution_stack_pointer: u64,
    trap_stack_pointer: u64,
) -> ! {
    // MAIR: Memory Attribute Indirection Register
    // Index 0: Normal memory, Write-Back, Read/Write Allocate
    // Index 1: Device-nGnRnE memory
    #[expect(clippy::identity_op)]
    let mair: u64 = 0xFF | (0x00 << 8);

    // ═══════════════════════════════════════════════════════════
    // STEP 1: Configure EL2 to allow EL1 operation
    // ═══════════════════════════════════════════════════════════

    // Set Hypervisor Configuration Register (EL2)
    // Set EL1 execution state to AArch64
    // @todo Explain the SWIO bit (SWIO hardwired on Pi3)
    HCR_EL2.write(HCR_EL2::RW::EL1IsAarch64 + HCR_EL2::SWIO::SET);
    // @todo disable VM bit to prevent stage 2 MMU translations

    configure_fp_simd_traps();
    configure_el0_visible_state();

    // ═══════════════════════════════════════════════════════════
    // STEP 2: Set up VBAR_EL1 (Exception Vector Base Address)
    // ═══════════════════════════════════════════════════════════

    assert!(
        vbar.trailing_zeros() >= 11,
        "Vector table address {vbar} is NOT properly aligned!"
    );

    // The address must be 2KB aligned (bits [10:0] must be 0).
    // We set the virtual address here since VBAR_EL1 is only
    // used after MMU is enabled (exceptions before ERET would
    // be taken at EL2, not EL1).
    VBAR_EL1.set(vbar);

    // Force VBAR update to complete before next instruction.
    barrier::isb(barrier::SY);

    liblog::info!("[!] Exception traps set up");

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

    // Set Saved Program Status Register (EL2)
    // Set up a simulated exception return.
    //
    // Fake a saved program status, where all interrupts were
    // masked and SP_EL1 was used as a stack pointer.
    // Implementation status: trusted fixture execution now returns to EL1t
    // using SP_EL0; exception entry selects the separate shared SP_EL1.
    SPSR_EL2.write(
        SPSR_EL2::D::Masked
            + SPSR_EL2::A::Masked
            + SPSR_EL2::I::Masked
            + SPSR_EL2::F::Masked
            + SPSR_EL2::M::EL1t, // Use SP_EL0, Return to EL1
    );

    // TODO: Mask interrupts in EL1
    // Implementation status: SPSR_EL2 above supplies the initial EL1 masks;
    // this saved EL1 status is overwritten on the first exception entry.
    SPSR_EL1.write(
        SPSR_EL1::D::Masked
            + SPSR_EL1::A::Masked
            + SPSR_EL1::I::Masked
            + SPSR_EL1::F::Masked
            + SPSR_EL1::M::EL1h, // Use SP_EL1
    );

    // Set return address and stack
    ELR_EL2.set(entry_point);
    SP_EL0.set(execution_stack_pointer);
    SP_EL1.set(trap_stack_pointer);

    memory_barrier();

    // ═══════════════════════════════════════════════════════════
    // STEP 5: Drop to EL1
    // ═══════════════════════════════════════════════════════════
    asm::eret() // FIXME this doesn't pass DTB (and we hopefully don't need it anymore)
}

/// Invalidate all TLB entries
#[inline(always)]
pub fn tlb_invalidate_all() {
    // SAFETY: Unsafe
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
