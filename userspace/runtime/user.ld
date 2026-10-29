/*
 * Link layout for EL0 userspace components.
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 *
 * Each component runs in its own AddressSpace, so all of them share one link
 * base. Every segment starts on a page so the loader can map the bundled
 * pages in place as Frames with per-segment permissions; .bss is allocated
 * separately. .exports holds the PPC export records, .driver_compatible a
 * driver's compatible strings, .driver_threads the entries of its further
 * Threads and .entry_priority its first Thread's priority, all read by
 * vesper-image-build.
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

    /* A driver's NUL-separated device tree `compatible` strings */
    .driver_compatible :
    {
        KEEP(*(.driver_compatible))
    }

    /* The entry addresses of a driver's further Threads */
    .driver_threads : ALIGN(8)
    {
        KEEP(*(.driver_threads))
    }

    /* The priority level of a component's first Thread, one byte */
    .entry_priority :
    {
        KEEP(*(.entry_priority))
    }

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
