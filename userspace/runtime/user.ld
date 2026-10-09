/*
 * Link layout for EL0 userspace components.
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 *
 * Each component runs in its own AddressSpace, so all of them share one link
 * base. Every segment starts on a page so the loader can map the bundled
 * pages in place as Frames with per-segment permissions; .bss is allocated
 * separately. .exports holds the PPC export records vesper-image-build reads.
 */

ENTRY(_start)

__COMPONENT_BASE = 0x400000;

SECTIONS
{
    . = __COMPONENT_BASE;

    .text : ALIGN(4K)
    {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        *(.bss .bss.*)
        *(COMMON)
    }

    .exports : ALIGN(8)
    {
        KEEP(*(.exports))
    }

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
