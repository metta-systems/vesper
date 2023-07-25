/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

__phys_load_addr = 0x80000;
ENTRY(__phys_load_addr);

PAGE_SIZE = 65536;

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
    segment_code            PT_LOAD FLAGS(5);
    segment_data            PT_LOAD FLAGS(6);
}

/* Symbols between __BOOT_START and __BOOT_END should be dropped after init is complete.
   Symbols between __RO_START and __RO_END are the kernel code.
   Symbols between __BSS_START and __BSS_END must be initialized to zero by startup code in the kernel.
*/
SECTIONS
{
    __STACK_START = __phys_load_addr; /* Stack grows from here towards 0x0. */
    . = __phys_load_addr; /* AArch64 boot address is 0x80000, 4K-aligned */

    /***********************************************************************************************
    * Code + RO Data
    ***********************************************************************************************/

    .text :
    {
        /*******************************************************************************************
        * Boot Code + Boot Data
        *******************************************************************************************/

        __BOOT_START = .;
        KEEP(*(.text.main.entry))
        *(.text.boot)
        *(.data.boot)
        . = ALIGN(PAGE_SIZE); /* Here the boot code ends */
        __BOOT_END = .; // __BOOT_END must be PAGE_SIZE aligned

        /*******************************************************************************************
        * Regular Kernel Code
        *******************************************************************************************/

        __RO_START = .;
        *(.text*)
    } :segment_code

    .vectors :
    {
        . = ALIGN(2048);
        __EXCEPTION_VECTORS_START = .;
        KEEP(*(.vectors))
    } :segment_code

    .rodata :
    {
        . = ALIGN(4);
        *(.rodata*)
        FILL(0x00)
        . = ALIGN(PAGE_SIZE); /* Fill up to page size */
        __RO_END = .; /* __RO_END must be PAGE_SIZE aligned */
    } :segment_code

    /***********************************************************************************************
    * Data + BSS
    ***********************************************************************************************/

    .data :
    {
        __DATA_START = .; /* __DATA_START must be PAGE_SIZE aligned */
        *(.data*)
        FILL(0x00)
    } :segment_data

    .bss (NOLOAD):
    {
        . = ALIGN(PAGE_SIZE);
        __BSS_START = .;
        *(.bss*)
        . = ALIGN(PAGE_SIZE); /* Align up to page size */
        __BSS_END = .;
        __BSS_SIZE_U64S = (__BSS_END - __BSS_START) / 8;
    } :segment_data

    /***********************************************************************************************
    * Misc
    ***********************************************************************************************/

    .got : { *(.got*) }
    ASSERT(SIZEOF(.got) == 0, "Relocation support not expected")

    /DISCARD/ : { *(.comment*) *(.gnu*) *(.note*) *(.eh_frame*) *(.text.chainboot*) }
}

INCLUDE linker/aarch64-exceptions.ld
