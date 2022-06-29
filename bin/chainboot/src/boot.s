// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2021 Andre Richter <andre.o.richter@gmail.com>
// Modifications
// Copyright (c) 2021- Berkus <berkus+github@metta.systems>

//--------------------------------------------------------------------------------------------------
// Definitions
//--------------------------------------------------------------------------------------------------

// Load the address of a symbol into a register, PC-relative.
//
// The symbol must lie within +/- 4 GiB of the Program Counter.
//
// # Resources
//
// - https://sourceware.org/binutils/docs-2.36/as/AArch64_002dRelocations.html
.macro ADR_REL register, symbol
    adrp	\register, \symbol
    add	\register, \register, #:lo12:\symbol
.endm

// Load the address of a symbol into a register, absolute.
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
.section .text._start

//------------------------------------------------------------------------------
// fn _start()
//------------------------------------------------------------------------------
// x0 contains DTB address on entry, preserve it until the call to Rust.
_start:
    // Only proceed on the boot core. Park it otherwise.
    mrs	x1, MPIDR_EL1
    and	x1, x1, 0b11          // core id mask
    cmp	x1, 0                 // boot core id
    b.ne	.L_parking_loop

    // If execution reaches here, it is the boot core.

    // Initialize bss.
    ADR_ABS	x1, __bss_start
    ADR_ABS x2, __bss_end_exclusive

.L_bss_init_loop:
    cmp	x1, x2
    b.eq	.L_relocate_binary
    stp	xzr, xzr, [x1], #16
    b	.L_bss_init_loop

    // Next, relocate the binary.
.L_relocate_binary:
    ADR_REL	x1, __binary_nonzero_lma           // The address the binary got loaded to.
    ADR_ABS	x2, __binary_nonzero_vma           // The address the binary was linked to.
    ADR_ABS	x3, __binary_nonzero_vma_end_exclusive
    sub x4, x2, x1                             // Get difference between vma and lma as max size

.L_copy_loop:
    ldr	x5, [x1], #8
    str	x5, [x2], #8
    cmp	x2, x3
    b.lo	.L_copy_loop

    // Prepare the jump to Rust code.
    // Set the stack pointer.
    ADR_ABS	x1, __rpi_phys_binary_load_addr
    mov	sp, x1

    // Pass DTB location in x0 to Rust init function.
    // Pass maximum kernel size as an argument in x1 to Rust init function.
    mov x1, x4

    // Jump to the relocated Rust code.
    ADR_ABS	x2, _start_rust
    br	x2

    // Infinitely wait for events (aka "park the core").
.L_parking_loop:
    wfe
    b	.L_parking_loop

.size	_start, . - _start
.type	_start, function
.global	_start