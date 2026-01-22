/* init_thread/init.ld - Identity-mapped init thread */

ENTRY(_start)

/* Physical load address - typical for RPi4 */
INIT_BASE = 0x80000;

SECTIONS
{
    . = INIT_BASE;

    __init_start = .;

    /* Entry point and early code */
    .text.boot ALIGN(4K) :
    {
        *(.text.boot)
    }

    .text ALIGN(4K) :
    {
        *(.text .text.*)
    }

    .rodata ALIGN(4K) :
    {
        *(.rodata .rodata.*)

        /* Embedded kernel will be here via include_bytes! */
    }

    .data ALIGN(4K) :
    {
        *(.data .data.*)
    }

    .bss ALIGN(4K) :
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    . = ALIGN(4K);
    __init_end = .;

    /* Stack for init thread (grows down) */
    . = ALIGN(16);
    __stack_bottom = .;
    . += 64K;
    __stack_top = .;

    /* Page tables - we allocate space for them here */
    . = ALIGN(4K);
    __page_tables_start = .;
    . += 256K;  /* Space for page tables */
    __page_tables_end = .;

    /* Free memory starts here - used for kernel placement */
    . = ALIGN(2M);  /* 2MB aligned for huge pages if desired */
    __free_memory_start = .;

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
