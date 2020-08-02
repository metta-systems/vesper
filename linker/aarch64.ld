/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

ENTRY(_boot_cores);

/* Symbols between __boot_start and __boot_end should be dropped after init is complete.
   Symbols between __ro_start and __ro_end are the kernel code.
   Symbols between __bss_start and __bss_end must be initialized to zero by r0 code in kernel.
*/
SECTIONS
{
    . = 0x80000; /* AArch64 boot address is 0x80000, 4K-aligned */
    __boot_start = .;
    .text :
    {
        KEEP(*(.text.boot))
        . = ALIGN(4096);
        KEEP(*(.data.boot))
        . = ALIGN(4096); /* Here boot code ends */
        __boot_end = .; // __boot_end must be 4KiB aligned
        __ro_start = .;
        *(.text .text.*)
    }

    .vectors ALIGN(2048):
    {
        *(.vectors)
    }

    .rodata ALIGN(4):
    {
        *(.rodata .rodata.*)
        FILL(0x00)
    }
    . = ALIGN(4096); /* Fill up to 4KiB */
    __ro_end = .; /* __ro_end must be 4KiB aligned */
    __data_start = .; /* __data_start must be 4KiB aligned */

    .data : /* @todo align data to 4K -- it's already aligned up to __ro_end marker now */
    {
        *(.data .data.*)
        FILL(0x00)
    }

    /* @todo could insert .data.boot here with proper alignment */

    .bss ALIGN(8):
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4096); /* Align up to 4KiB */
        __bss_end = .;
    }

    /DISCARD/ : { *(.comment) *(.gnu*) *(.note*) *(.eh_frame*) }
}

PROVIDE(current_el0_synchronous   = default_exception_handler);
PROVIDE(current_el0_irq           = default_exception_handler);
PROVIDE(current_el0_fiq           = default_exception_handler);
PROVIDE(current_el0_serror        = default_exception_handler);

PROVIDE(current_elx_synchronous   = default_exception_handler);
PROVIDE(current_elx_irq           = default_exception_handler);
PROVIDE(current_elx_fiq           = default_exception_handler);
PROVIDE(current_elx_serror        = default_exception_handler);

PROVIDE(lower_aarch64_synchronous = default_exception_handler);
PROVIDE(lower_aarch64_irq         = default_exception_handler);
PROVIDE(lower_aarch64_fiq         = default_exception_handler);
PROVIDE(lower_aarch64_serror      = default_exception_handler);

PROVIDE(lower_aarch32_synchronous = default_exception_handler);
PROVIDE(lower_aarch32_irq         = default_exception_handler);
PROVIDE(lower_aarch32_fiq         = default_exception_handler);
PROVIDE(lower_aarch32_serror      = default_exception_handler);
