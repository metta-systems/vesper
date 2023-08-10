/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

PAGE_SIZE = 64K;
PAGE_MASK = PAGE_SIZE - 1;

__phys_mem_start = 0x0;

__phys_load_addr = 0x80000;
ENTRY(__phys_load_addr);

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
   Symbols between __CODE_START and __CODE_END are the kernel code.
   Symbols between __BSS_START and __BSS_END must be initialized to zero by startup code in the kernel.
*/
SECTIONS
{
    . = __phys_mem_start;

    /***********************************************************************************************
    * Boot Core Stack
    ***********************************************************************************************/
    .boot_core_stack (NOLOAD) :
    {
         __STACK_BOTTOM = .;                 /*   ^             */
                                             /*   | stack       */
        . = __phys_load_addr;                /*   | growth      AArch64 boot address is 0x80000, 4K-aligned */
                                             /*   | direction   */
        __STACK_TOP = .;                     /*   | Stack grows from here towards 0x0. */
    } :segment_boot_core_stack

    ASSERT((. & PAGE_MASK) == 0, "End of boot core stack is not page aligned")

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
        . = ALIGN(PAGE_SIZE);
        __BOOT_END = .; /* Here the boot code ends */
        ASSERT((__BOOT_END & PAGE_MASK) == 0, "End of boot code is not page aligned")

        /*******************************************************************************************
        * Regular Kernel Code
        *******************************************************************************************/

        __CODE_START = .;
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
        __CODE_END = .;
        ASSERT((__CODE_END & PAGE_MASK) == 0, "End of kernel code is not page aligned")
    } :segment_code

    /***********************************************************************************************
    * Data + BSS
    ***********************************************************************************************/

    .data :
    {
        __DATA_START = .;
        ASSERT((__DATA_START & PAGE_MASK) == 0, "Start of kernel data is not page aligned")
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

    __DATA_END = .;

    /***********************************************************************************************
    * MMIO Remap Reserved
    ***********************************************************************************************/
    __MMIO_REMAP_START = .;
    . += 8 * 1024 * 1024;
    __MMIO_REMAP_END = .;

    ASSERT((. & PAGE_MASK) == 0, "MMIO remap reservation is not page aligned")

    /***********************************************************************************************
    * Misc
    ***********************************************************************************************/

    .got : { *(.got*) }
    ASSERT(SIZEOF(.got) == 0, "Relocation support not expected")

    /DISCARD/ : { *(.comment*) *(.gnu*) *(.note*) *(.eh_frame*) *(.text.chainboot*) }
}

INCLUDE machine/src/arch/aarch64/linker/aarch64-exceptions.ld
