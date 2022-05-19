/* SPDX-License-Identifier: MIT OR Apache-2.0
 *
 * Copyright (c) 2018-2021 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) 2021- Berkus <berkus+github@metta.systems>
 */

/*
 * Information from:
 * [Output Section Address](https://sourceware.org/binutils/docs/ld/Output-Section-Address.html)
 * [Output Section LMA](https://sourceware.org/binutils/docs/ld/Output-Section-LMA.html)
 * [Output Section Attributes](https://sourceware.org/binutils/docs/ld/Output-Section-Attributes.html#Output-Section-Attributes)
 */

/* The physical address at which the the kernel binary will be loaded by the Raspberry's firmware */
__rpi_phys_binary_load_addr = 0x80000;


ENTRY(__rpi_phys_binary_load_addr)

/* Flags:
 *     4 == R
 *     5 == RX
 *     6 == RW
 *
 * Segments are marked PT_LOAD below so that the ELF file provides virtual and physical addresses.
 * It doesn't mean all of them need actually be loaded.
 */
PHDRS
{
    segment_boot_core_stack PT_LOAD FLAGS(6);
    segment_start_code      PT_LOAD FLAGS(5);
    segment_code            PT_LOAD FLAGS(5);
    segment_data            PT_LOAD FLAGS(6);
}

SECTIONS
{
    /***********************************************************************************************
    * Boot Core Stack
    ***********************************************************************************************/
    .boot_core_stack (NOLOAD) :
    {
                                             /*   ^             */
                                             /*   | stack       */
        . += __rpi_phys_binary_load_addr;    /*   | growth      */
                                             /*   | direction   */
        __boot_core_stack_end_exclusive = .; /*   |             */
    } :segment_boot_core_stack

    . = __rpi_phys_binary_load_addr;

    .text :
    {
        KEEP(*(.text._start))
    } :segment_start_code

    /* Align to 8 bytes, b/c relocating the binary is done in u64 chunks */
    . = ALIGN(8);

    __binary_nonzero_lma = .;

    /* Set the link address to 32 MiB */
    /* This dictates the max size of the loadable kernel. */
    . += 0x2000000;

    /***********************************************************************************************
    * Code + RO Data + Global Offset Table
    ***********************************************************************************************/
    __binary_nonzero_vma = .;
    .text : AT (ADDR(.text) + SIZEOF(.text))
    {
        *(.text._start_rust)      /* The Rust entry point */
        /* *(text.memcpy) -- only relevant for Rust relocator impl which is currently impossible */
        *(.text*)                 /* Everything else */
    } :segment_code

    .rodata : ALIGN(8) { *(.rodata*) } :segment_code
    .got    : ALIGN(8) { *(.got)     } :segment_code

    /***********************************************************************************************
    * Data + BSS
    ***********************************************************************************************/
    .data : { *(.data*) } :segment_data

    /* Fill up to 8 bytes, b/c relocating the binary is done in u64 chunks */
    . = ALIGN(8);
    __binary_nonzero_vma_end_exclusive = .;

    /* Section is zeroed in pairs of u64. Align start and end to 16 bytes */
    .bss (NOLOAD) : ALIGN(16)
    {
        __bss_start = .;
        *(.bss*);
        . = ALIGN(16);
        __bss_end_exclusive = .;
    } :segment_data
}
