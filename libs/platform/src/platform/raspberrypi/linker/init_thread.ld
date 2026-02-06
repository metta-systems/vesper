/* init_thread/init.ld - Identity-mapped init thread */

ENTRY(_start)

/* Physical load address - typical for RPi4 */
INIT_BASE = 0x80000;

SECTIONS
{
    /* Stack for init thread (grows down) */
    . = 0;
    __stack_bottom = .;
    . = INIT_BASE;
    __stack_top = .;

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
