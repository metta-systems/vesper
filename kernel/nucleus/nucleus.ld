/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

__PAGE_SIZE = 64K;
__PAGE_MASK = __PAGE_SIZE - 1;

__KERNEL_VIRT_ADDR_BASE = 0xffff800000000000; /* Nucleus is mapped here, init_thread is identity-mapped */

/* Flags:
 *     4 == R
 *     5 == RX
 *     6 == RW
 *
 * Segments are marked PT_LOAD below so that the ELF file provides virtual and physical addresses.
 * It doesn't mean all of them need actually be loaded.
 */
PHDRS
{
    segment_code            PT_LOAD FLAGS(5);
    segment_data            PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = __KERNEL_VIRT_ADDR_BASE;

    /* __nucleus_start = .; */

    .text : /*AT(ADDR(.text) - __KERNEL_VIRT_ADDR_BASE)*/ ALIGN(__PAGE_SIZE)
    {
        /*******************************************************************************************
        * Regular Kernel Code
        *******************************************************************************************/
        *(.text*)
        . = ALIGN(2K);
        KEEP(*(.vectors))
        . = ALIGN(2K);
    } :segment_code

    /* TODO: rodata is currently executable, but doesn't have to... also each individual section must be aligned to 4K at least for mappings to work */
    .rodata : /*AT(ADDR(.rodata) - __KERNEL_VIRT_ADDR_BASE)*/ ALIGN(__PAGE_SIZE)
    {
        *(.rodata*)
        FILL(0x00)
        . = ALIGN(__PAGE_SIZE); /* Fill up to page size */
    } :segment_code

    /* __CODE_END = .;
    ASSERT((__CODE_END & __PAGE_MASK) == 0, "End of kernel code is not page aligned") */

    /***********************************************************************************************
    * Data + BSS
    ***********************************************************************************************/

    .data : /*AT(ADDR(.data) - __KERNEL_VIRT_ADDR_BASE)*/ ALIGN(__PAGE_SIZE)
    {
        /* __DATA_START = .;
        ASSERT((__DATA_START & __PAGE_MASK) == 0, "Start of kernel data is not page aligned") */

        *(.data*)
        FILL(0x00)

        . = ALIGN(__PAGE_SIZE);
    } :segment_data

    .bss (NOLOAD) : /*AT(ADDR(.bss) - __KERNEL_VIRT_ADDR_BASE)*/ ALIGN(__PAGE_SIZE)
    {
        /* __BSS_START = LOADADDR(.bss); LMA (PhysAddr) - for the loader in init_thread */
        *(.bss*)
        *(COMMON)
        . = ALIGN(__PAGE_SIZE); /* Align up to page size */
    } :segment_data

    /* __BSS_END = __BSS_START + ALIGN(SIZEOF(.bss), __PAGE_SIZE); *//* LMA, Physical address */
    /* __DATA_END = .; *//* VMA, Virtual address */

    /***********************************************************************************************
    * MMIO Remap Reserved (8Mb)
    ***********************************************************************************************/
    /* __MMIO_REMAP_START = .; FIXME: VMA? Must be LMA perhaps, the point is to remap this
    . += 8 * 1024 * 1024;
    __MMIO_REMAP_END = .; FIXME: VMA? Must be LMA perhaps, the point is to remap this */

    /* ASSERT((. & __PAGE_MASK) == 0, "MMIO remap reservation is not page aligned")
    __kernel_virt_addr_space_size = ABSOLUTE(. - __KERNEL_VIRT_ADDR_BASE); */

    /* Physical end */
    /* __nucleus_end = __nucleus_start + __kernel_virt_addr_space_size;
    __kernel_end = .; */

    /***********************************************************************************************
    * Misc
    ***********************************************************************************************/

    .got : { *(.got*) }
    ASSERT(SIZEOF(.got) == 0, "Unexpected relocations in kernel binary.")

    /DISCARD/ : {
        *(.comment*)
        *(.gnu*)
        *(.note*)
        *(.eh_frame*)
        *(.text.chainboot*)
    }
}

PROVIDE(current_el0_synchronous   = current_el0_synchronous);
PROVIDE(current_el0_irq           = current_el0_irq);
PROVIDE(current_el0_fiq           = default_exception_handler);
PROVIDE(current_el0_serror        = current_el0_serror);

PROVIDE(current_elx_synchronous   = current_elx_synchronous);
PROVIDE(current_elx_irq           = current_elx_irq);
PROVIDE(current_elx_fiq           = default_exception_handler);
PROVIDE(current_elx_serror        = current_elx_serror);

PROVIDE(lower_aarch64_synchronous = lower_aarch64_synchronous);
PROVIDE(lower_aarch64_irq         = lower_aarch64_irq);
PROVIDE(lower_aarch64_fiq         = default_exception_handler);
PROVIDE(lower_aarch64_serror      = lower_aarch64_serror);

PROVIDE(lower_aarch32_synchronous = lower_aarch32_synchronous);
PROVIDE(lower_aarch32_irq         = lower_aarch32_irq);
PROVIDE(lower_aarch32_fiq         = default_exception_handler);
PROVIDE(lower_aarch32_serror      = lower_aarch32_serror);
