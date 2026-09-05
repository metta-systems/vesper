// SPDX-License-Identifier: BlueOak-1.0.0
// Copyright (c) Berkus Decker <berkus+vesper@metta.systems>

//
// Pre-boot code.
// Used only because Rust's abstract machine considers UB any access to statics
// before statics have been initialized. This is exactly the case for the boot code.
// So we avoid referencing any linker symbol statics from the Rust code, and delegate the
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
.section .text.main.entry

//------------------------------------------------------------------------------
/// Entrypoint of the processor.
///
/// Parks all cores except core0 and checks if we started in EL2/EL3. If
/// so, init BSS and enter Rust code.
///
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel main function in Rust.
///
/// Dissection of various RPi core boot stubs is available
/// [here](https://leiradel.github.io/2019/01/20/Raspberry-Pi-Stubs.html).
///
/// x0 contains DTB address on entry, preserve it until the call to Rust.
//------------------------------------------------------------------------------
_boot_cores:
    // Only proceed on the boot core. Park it otherwise.
    mrs x1, MPIDR_EL1
    and x1, x1, {CONST_CORE_ID_MASK}
    mov x2, {CONST_BOOT_CORE_ID}
    cmp x1, x2
    b.ne .L_parking_loop

    // If execution reaches here, it is the boot core.

    // Initialize BSS - prepare to fearlessly call into Rust code.
    // Assumptions: BSS start is u128-aligned, BSS end is u128-aligned.
    // __BSS_START and __BSS_END are defined in the linker script
    ADR_REL x1, __BSS_START // must be physical address!!!1
    ADR_REL x2, __BSS_END   // must be physical address!!!1
.L__bss_init_loop:
    cmp x1, x2
    b.eq .L_setup_stack
    stp xzr, xzr, [x1], #16
    b .L__bss_init_loop

.L_setup_stack:
    ADR_ABS x1, __STACK_TOP
    mov sp, x1

    // On entry, x0 contains DTB address
    bl _startup_in_rust

.L_parking_loop:
    wfe
    b .L_parking_loop

.size _boot_cores, . - _boot_cores
.type _boot_cores, function
.global _boot_cores
