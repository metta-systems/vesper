/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

ENTRY(_boot_cores);

PAGE_SIZE = 65536;

/* Symbols between __BOOT_START and __BOOT_END should be dropped after init is complete.
   Symbols between __RO_START and __RO_END are the kernel code.
   Symbols between __BSS_START and __BSS_END must be initialized to zero by startup code in the kernel.
*/
SECTIONS
{
    . = 0x80000; /* AArch64 boot address is 0x80000, 4K-aligned */
    __STACK_START = 0x80000; /* Stack grows from here towards 0x0. */
    __BOOT_START = .;
    .text :
    {
        KEEP(*(.text.main.entry)) // Entry point must go first
        *(.text.boot)
        //. = ALIGN(PAGE_SIZE);
        *(.data.boot)
        . = ALIGN(PAGE_SIZE); /* Here the boot code ends */
        __BOOT_END = .; // __BOOT_END must be PAGE_SIZE aligned
        __RO_START = .;
        *(.text .text.*)
    }

    .vectors ALIGN(2048):
    {
        KEEP(*(.vectors))
    }

    .rodata ALIGN(4):
    {
        *(.rodata .rodata.*)
        FILL(0x00)
    }
    . = ALIGN(PAGE_SIZE); /* Fill up to page size */
    __RO_END = .; /* __RO_END must be PAGE_SIZE aligned */
    __DATA_START = .; /* __DATA_START must be PAGE_SIZE aligned */

    .data : /* @todo align data to 4K -- it's already aligned up to __RO_END marker now */
    {
        *(.data .data.*)
        FILL(0x00)
    }

    /* @todo could insert .data.boot here with proper alignment */

    .bss ALIGN(8) (NOLOAD):
    {
        __BSS_START = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(PAGE_SIZE); /* Align up to page size */
        __BSS_SIZE = . - __BSS_START;
        /* __BSS_END = .; unused */
    }

    /DISCARD/ : { *(.comment) *(.gnu*) *(.note*) *(.eh_frame*) *(.text.chainboot*) }
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
