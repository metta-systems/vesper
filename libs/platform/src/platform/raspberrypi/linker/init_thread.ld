/*
 * Identity-mapped init thread.
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

ENTRY(_boot_cores)

/* Physical load address - typical for RPi4 */
__INIT_BASE = 0x80000;

SECTIONS
{
    /* Stack for init thread (grows down) */
    . = 4K;
    __STACK_BOTTOM = .;
    . = __INIT_BASE;
    __STACK_TOP = .;

    __INIT_START = .;

    .text : ALIGN(4K)
    {
        *(.text.main.entry) /* Entry point and early code */
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)

        /* Embedded kernel will be here via include_bytes! */
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        __BSS_START = .;
        *(.bss .bss.*)
        *(COMMON)
        __BSS_END = .;
    }

    . = ALIGN(4K);
    __INIT_END = .;

    /* Free memory starts here - used for kernel placement */
    . = ALIGN(2M);  /* 2MB aligned for huge pages if desired */
    __FREE_MEMORY_START = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
