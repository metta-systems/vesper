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
 *
 * Symbols referenced from Rust code are __IN_CAPS, following rust conventions, symbols that are used only in linker script
 * and from assembly code __are_in_lowercase.
 */

/* The physical address at which the the kernel binary will be loaded by the Raspberry's firmware */
__rpi_phys_binary_load_addr = 0x80000;


ENTRY(__rpi_phys_binary_load_addr)

/* Flags:
 * R = 4
 * RX = 5
 * RW = 6
 *
 * Segments are marked PT_LOAD below so that the ELF file provides virtual and physical addresses.
 * It doesn't mean all of them need actually be loaded.
 */
PHDRS
{
    segment_boot_core_stack PT_LOAD FLAGS(6); /* RW */
    segment_start_code      PT_LOAD FLAGS(5); /* RX */
    segment_code            PT_LOAD FLAGS(5); /* RX */
    segment_data            PT_LOAD FLAGS(6); /* RW */
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
        /* .text.chainboot.entry is an assembly piece linked at 0x80000 which */
        /* relocates the rest of the code to the address above 32Mb */
        KEEP(*(.text.chainboot.entry))
        /* Align to 16 bytes, b/c relocating the binary is done in u128 chunks */
        . = ALIGN(16);
    } :segment_start_code

    /* Physical code location in file */
    __binary_nonzero_lma = .;

    /* Set the link address to 32 MiB */
    /* This dictates the max size of the loadable kernel. */
    . += 0x2000000;

    /***********************************************************************************************
    * Code + Exception Vectors + RO Data + Global Offset Table
    ***********************************************************************************************/
    /* Logical code location in memory */
    __binary_nonzero_vma = .;
    .text : AT (ADDR(.text) + SIZEOF(.text)) ALIGN(16)
    {
        *(.text*)                 /* The Rust entry point and everything else */
    } :segment_code

    . = ALIGN(2048);

    .vectors : ALIGN(2048)
    {
        __EXCEPTION_VECTORS_START = .;
        KEEP(*(.vectors))
    } :segment_code

    .rodata : ALIGN(8) { *(.rodata*) } :segment_code
    .got    : ALIGN(8) { *(.got)     } :segment_code

    /***********************************************************************************************
    * Data + BSS
    ***********************************************************************************************/
    .data : { *(.data*) } :segment_data

    /* Align to 16 bytes, b/c relocating the binary is done in u128 chunks */
    . = ALIGN(16);
    __binary_nonzero_vma_end_exclusive = .;

    /* Align to 16 bytes, b/c BSS section is zeroed in u1238 chunks. */
    .bss (NOLOAD) : ALIGN(16)
    {
        __BSS_START = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __BSS_END = .;
    } :segment_data

    /DISCARD/ : { *(.comment) *(.gnu*) *(.note*) *(.eh_frame*) *(.text.boot*)}
}

INCLUDE libs/exception/src/arch/aarch64/linker/aarch64-exceptions.ld
