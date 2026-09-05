/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

__PAGE_SIZE = 4K;
__PAGE_MASK = __PAGE_SIZE - 1;

__KERNEL_VIRT_ADDR_BASE = 0xffff800000000000; /* Nucleus is higher-mem mapped here */

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
    segment_code            PT_LOAD FLAGS(5);
    segment_rodata          PT_LOAD FLAGS(4);
    segment_data            PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = __KERNEL_VIRT_ADDR_BASE;

    /**********************************************************************************************
    * Regular Kernel Code
    **********************************************************************************************/
    .text : ALIGN(__PAGE_SIZE)
    {
        . = ALIGN(__PAGE_SIZE);
        *(.text*)
        . = ALIGN(2K);
        KEEP(*(.vectors))
    } :segment_code

    /**********************************************************************************************
    * Read-only data
    **********************************************************************************************/
    .rodata : ALIGN(__PAGE_SIZE)
    {
        . = ALIGN(__PAGE_SIZE);
        *(.rodata*)
        FILL(0x00)
    } :segment_rodata

    /**********************************************************************************************
    * Data + BSS
    **********************************************************************************************/
    .data : ALIGN(__PAGE_SIZE)
    {
        . = ALIGN(__PAGE_SIZE);
        *(.data*)
    } :segment_data

    .bss (NOLOAD) : ALIGN(__PAGE_SIZE)
    {
        . = ALIGN(__PAGE_SIZE);
        *(.bss*)
        *(COMMON)
    } :segment_data

    . = ALIGN(__PAGE_SIZE);
    . += __PAGE_SIZE; /* add guard page between BSS and the stack! */

    /* Kernel stack virt addr will be mapped beginning from here */
    __STACK_VIRT_BOTTOM = .;

    /***********************************************************************************************
    * MMIO Remap Reserved (8Mb) - TODO: we do not use, instead map it via bump allocator
    ***********************************************************************************************/
    /* __MMIO_REMAP_START = .; FIXME: VMA? Must be LMA perhaps, the point is to remap this
    . += 8M;
    __MMIO_REMAP_END = .; FIXME: VMA? Must be LMA perhaps, the point is to remap this */

    /***********************************************************************************************
    * Misc
    ***********************************************************************************************/

    .got : { *(.got*) }
    ASSERT(SIZEOF(.got) == 0, "Unexpected relocations in kernel binary.")

    /DISCARD/ : {
        *(.comment*)
        *(.gnu*)
        *(.note*)
        *(.eh_frame*)
        *(.text.chainboot*)
    }
}

INCLUDE libs/exception/src/arch/aarch64/linker/aarch64-exceptions.ld
