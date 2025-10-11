// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2021 Andre Richter <andre.o.richter@gmail.com>
// Modifications
// Copyright (c) 2021- Berkus <berkus+github@metta.systems>

//
// Pre-boot code.
// Used only because Rust's abstract machine considers UB any access to statics
// before statics have been initialized. This is exactly the case for the boot code.
// So we avoid referencing any statics in the Rust code, and delegate the
// task to assembly piece instead.
//

//--------------------------------------------------------------------------------------------------
// Definitions
//--------------------------------------------------------------------------------------------------

// Load the PC-relative address of a symbol into a register.
//
// The symbol must lie within +/- 4 GiB of the Program Counter.
//
// # Resources
//
// - https://sourceware.org/binutils/docs-2.36/as/AArch64_002dRelocations.html
.macro ADR_REL register, symbol
    adrp \register, \symbol
    add \register, \register, #:lo12:\symbol
.endm

// Load the absolute address of a symbol into a register.
//
// # Resources
//
// - https://sourceware.org/binutils/docs-2.36/as/AArch64_002dRelocations.html
.macro ADR_ABS register, symbol
    movz	\register, #:abs_g2:\symbol
    movk	\register, #:abs_g1_nc:\symbol
    movk	\register, #:abs_g0_nc:\symbol
.endm

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------
.section .text.chainboot.entry

//------------------------------------------------------------------------------
// fn _start()
//
/// Reset function.
///
/// Initializes the bss section before calling into the user's `main()`.
//------------------------------------------------------------------------------
// x0 contains DTB address on entry, preserve it until the call to Rust.
_start:
    // Only proceed on the boot core. Park it otherwise.
    mrs x1, MPIDR_EL1
    and x1, x1, {CONST_CORE_ID_MASK}
    mov x2, {CONST_BOOT_CORE_ID}
    cmp x1, x2
    b.ne .L_parking_loop

    // If execution reaches here, it is the boot core.

    // Initialize BSS
    // Assumptions: BSS start is u128-aligned, BSS end is u128-aligned.
    // __BSS_START and __BSS_END are defined in linker script
    ADR_REL x1, __BSS_START
    ADR_REL x2, __BSS_END
.L__bss_init_loop:
    cmp x1, x2
    b.eq .L_relocate_binary
    stp xzr, xzr, [x1], #16
    b .L__bss_init_loop

    // Next, relocate the binary code from __binary_nonzero_lma to __binary_nonzero_vma
    // TODO: check that DTB doesn't overlay with target space.
.L_relocate_binary:
    ADR_REL x1, __binary_nonzero_lma           // The address the binary got loaded to.
    ADR_ABS x2, __binary_nonzero_vma           // The address the binary was linked to.
    ADR_ABS x3, __binary_nonzero_vma_end_exclusive

    // max loadable kernel size = VMA - SP
    sub x4, x2, x1                             // Get difference between vma and lma as max size

.L__relocate_loop:
    ldp x5, x6, [x1], #16
    stp x5, x6, [x2], #16
    cmp x2, x3
    b.lo .L__relocate_loop

    // Prepare the jump to Rust code.
    // Set the stack pointer.
    ADR_ABS x1, __rpi_phys_binary_load_addr
    mov sp, x1

    // Pass DTB location in x0 to Rust init function.
    // It's already in x0 since boot.
    // Pass maximum kernel size as an argument in x1 to Rust init function.
    mov x1, x4

     // Jump to the relocated Rust code.
    ADR_ABS x2, kernel_init
    br x2

    // Infinitely wait for events (aka "park the core").
.L_parking_loop:
    wfe
    b .L_parking_loop

.size _start, . - _start
.type _start, function
.global _start
