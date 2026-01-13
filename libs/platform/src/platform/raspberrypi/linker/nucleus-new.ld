/* Kernel linker script:
 * Link all init thread code first, everything that runs once and will be dropped.
 * Then put the kernel proper code and static data, aligned to the page size.
 * The init thread will map it to higher half.
 */

SECTIONS {
    . = 0;
    __init_start = .;

    /* Init stack - also reclaimable */
    __init_stack_bottom = .;
    .init.stack (NOLOAD) : {
        . += 64K;
    }
    __init_stack_top = .;

    /* Init-only sections - CAN be reclaimed */
    .init.text : {
        (.init_thread.text.entry)
        (*.init_thread.text)
        /* All Phase 0-5 code goes here */
    }

    .init.data : {
        (*.init_thread.data)
        *(.data.page_tables)  /* Initial page tables */
    }
    (*.init_thread.bss)

    . = ALIGN(4K);
    __init_end = .;

    . = KERNEL_VIRT_BASE;

    /* Regular kernel text - not reclaimable */
    .text : {
        *(.text.boot)      /* Very first code */
        *(.text .text.*)
    }

    .rodata : { *(.rodata .rodata.*) }
    .data   : { *(.data .data.*) }

    /* BSS */
    .bss (NOLOAD) : {
        __bss_start = .;
        *(.bss .bss.*)
        __bss_end = .;
    }

    __kernel_end = .;
}
