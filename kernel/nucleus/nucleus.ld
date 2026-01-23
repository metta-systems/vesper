/* Higher-half kernel with vector table */

/* ENTRY(_kernel_entry) */

KERNEL_VIRT_BASE = 0xFFFF000000000000;

SECTIONS
{
    . = KERNEL_VIRT_BASE;

    __kernel_start = .;

    /* Code section */
    .text : AT(ADDR(.text) - KERNEL_VIRT_BASE) ALIGN(4K)
    {
        __text_start = .;
        *(.text.entry)
        *(.text .text.*)
        . = ALIGN(2K);  /* Ensure full 2KB for vector table */
        KEEP(*(.vectors))
        . = ALIGN(2K);  /* Ensure full 2KB for vector table */
        __text_end = .;
    }

    /* Read-only data */
    .rodata : AT(ADDR(.rodata) - KERNEL_VIRT_BASE) ALIGN(4K)
    {
        __rodata_start = .;
        *(.rodata .rodata.*)
        __rodata_end = .;
    }

    /* Initialized data */
    .data : AT(ADDR(.data) - KERNEL_VIRT_BASE) ALIGN(4K)
    {
        __data_start = .;
        *(.data .data.*)
        __data_end = .;
    }

    /* BSS */
    .bss : AT(ADDR(.bss) - KERNEL_VIRT_BASE) ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    . = ALIGN(4K);
    __kernel_end = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
