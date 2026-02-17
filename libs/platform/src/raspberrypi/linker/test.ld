/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 *
 * Linker script for device test binaries (RPi3, QEMU).
 *
 * Test binaries use libboot::entry! which puts the boot stub in .text.main.entry
 * and expects __STACK_TOP, __STACK_BOTTOM, __BSS_START, __BSS_END symbols.
 * Layout mirrors init_thread.ld: load at 0x80000, identity-mapped.
 */

ENTRY(_boot_cores)

__TEST_BASE = 0x80000;

SECTIONS
{
    /* Stack for test binary (grows down toward 0x80000) */
    . = 4K;
    __STACK_BOTTOM = .;
    . = __TEST_BASE;
    __STACK_TOP = .;

    __TEST_START = .;

    .text : ALIGN(4K)
    {
        *(.text.main.entry) /* Boot stub - must be first */
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

    .bss : ALIGN(16)
    {
        __BSS_START = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __BSS_END = .;
    }

    . = ALIGN(4K);
    __TEST_END = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
        *(.gnu*)
    }
}
