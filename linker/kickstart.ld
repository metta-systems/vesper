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
    /* The low 512 KiB below the image stay the firmware's: the spin-table
     * release words of the Raspberry Pi 3 and 4 (0xd8..0xf0) and TF-A on the
     * Pi 5 (`/reserved-memory/atf@0`, 0..0x80000). The init stack is after
     * the image instead. */
    . = __INIT_BASE;

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

    /* Bundled userspace components: page-aligned, page-padded segments that
     * the builder maps in place as Frames (see vesper-image-build). */
    .components : ALIGN(4K)
    {
        __COMPONENTS_START = .;
        KEEP(*(.components))
        . = ALIGN(4K);
        __COMPONENTS_END = .;
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
        /* The boot loop clears 16-byte words and terminates at this symbol. */
        . = ALIGN(16);
        __BSS_END = .;
    }

    ASSERT((__BSS_START % 16) == 0, "BSS start must match the 16-byte boot clear stride")
    ASSERT((__BSS_END % 16) == 0, "BSS end must match the 16-byte boot clear stride")

    . = ALIGN(4K);
    __INIT_END = .;

    /* Stack for the init thread (grows down), outside the image so the image
     * and the stack can be mapped separately. Not loaded: the RAM after the
     * image file. */
    __STACK_BOTTOM = .;
    . += 512K;
    __STACK_TOP = .;

    /* Free memory starts here - used for kernel placement */
    . = ALIGN(2M);  /* 2MB aligned for huge pages if desired */
    __FREE_MEMORY_START = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
        /* Export tables matter only in userspace component images. */
        *(.exports)
    }
}
