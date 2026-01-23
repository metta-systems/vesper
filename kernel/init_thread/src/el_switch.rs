// init_thread/src/el_switch.rs - EL2 to EL1 transition with VBAR setup

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

    unsafe {
        core::arch::asm!(
            // ═══════════════════════════════════════════════════════════
            // STEP 1: Configure EL2 to allow EL1 operation
            // ═══════════════════════════════════════════════════════════

            // HCR_EL2: RW=1 means EL1 is AArch64
            "mov     x0, #(1 << 31)",
            "msr     hcr_el2, x0",

            // ═══════════════════════════════════════════════════════════
            // STEP 2: Set up VBAR_EL1 (Exception Vector Base Address)
            // ═══════════════════════════════════════════════════════════
            //
            // The address must be 2KB aligned (bits [10:0] must be 0).
            // We set the virtual address here since VBAR_EL1 is only
            // used after MMU is enabled (exceptions before ERET would
            // be taken at EL2, not EL1).

            "msr     vbar_el1, {vbar}",

            // ═══════════════════════════════════════════════════════════
            // STEP 3: Configure EL1 MMU settings
            // ═══════════════════════════════════════════════════════════

            "msr     mair_el1, {mair}",

            // TCR_EL1: Translation Control Register
            "mov     x0, #16",              // T0SZ = 16 (48-bit VA for TTBR0)
            "orr     x0, x0, #(16 << 16)",  // T1SZ = 16 (48-bit VA for TTBR1)
            "orr     x0, x0, #(0b10 << 30)", // TG1 = 4KB granule
            "orr     x0, x0, #(0b11 << 12)", // SH0 = Inner shareable
            "orr     x0, x0, #(0b11 << 28)", // SH1 = Inner shareable
            "orr     x0, x0, #(0b01 << 10)", // ORGN0 = Write-back
            "orr     x0, x0, #(0b01 << 26)", // ORGN1 = Write-back
            "orr     x0, x0, #(0b01 << 8)",  // IRGN0 = Write-back
            "orr     x0, x0, #(0b01 << 24)", // IRGN1 = Write-back
            "msr     tcr_el1, x0",

            "msr     ttbr0_el1, {ttbr0}",
            "msr     ttbr1_el1, {ttbr1}",

            // ═══════════════════════════════════════════════════════════
            // STEP 4: Prepare to drop to EL1 with MMU enabled
            // ═══════════════════════════════════════════════════════════

            // Enable MMU (takes effect after ERET)
            "mrs     x0, sctlr_el1",
            "orr     x0, x0, #1",           // M = 1
            "orr     x0, x0, #(1 << 2)",    // C = 1
            "orr     x0, x0, #(1 << 12)",   // I = 1
            "msr     sctlr_el1, x0",

            // SPSR_EL2: EL1h with DAIF masked
            "mov     x0, #0b0101",          // EL1h
            "orr     x0, x0, #(0b1111 << 6)", // Mask DAIF
            "msr     spsr_el2, x0",

            // Set return address and stack
            "msr     elr_el2, {entry}",
            "msr     sp_el1, {sp}",

            "dsb     sy",
            "isb",

            // ═══════════════════════════════════════════════════════════
            // STEP 5: Drop to EL1
            // ═══════════════════════════════════════════════════════════
            "eret",

            mair = in(reg) mair,
            ttbr0 = in(reg) ttbr0,
            ttbr1 = in(reg) ttbr1,
            vbar = in(reg) vbar,
            entry = in(reg) entry_point,
            sp = in(reg) stack_pointer,
            options(noreturn, nostack)
        );
    }
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
