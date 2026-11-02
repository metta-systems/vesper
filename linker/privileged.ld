/*
 * Linker script for privileged (EL1) components, e.g. interrupt controllers.
 *
 * Components are position-independent (`-C relocation-model=pie`, `--pie`),
 * linked at 0 and relocated by kickstart into the kernel half. Only
 * R_AARCH64_RELATIVE relocations are allowed (checked by vesper-image-build).
 *
 * Three page-aligned loadable segments: code (R-X), read-only data plus the
 * dynamic relocation tables (R--), and writable data (RW-), which holds the
 * relocated operation table and `.bss`.
 */

ENTRY(IRQCHIP_OPS)

PHDRS
{
    text    PT_LOAD    FLAGS(5);  /* R-X */
    rodata  PT_LOAD    FLAGS(4);  /* R-- */
    data    PT_LOAD    FLAGS(6);  /* RW- */
    dynamic PT_DYNAMIC FLAGS(6);
}

SECTIONS
{
    . = 0;

    .text : ALIGN(4096)
    {
        *(.text .text.*)
    } :text

    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.*) } :rodata
    .dynsym : { *(.dynsym) } :rodata
    .dynstr : { *(.dynstr) } :rodata
    .hash : { *(.hash) } :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .rela.dyn : { *(.rela.dyn .rela.*) } :rodata
    /* NUL-separated device tree `compatible` strings, read by vesper-image-build */
    .irqchip_compatible : { KEEP(*(.irqchip_compatible)) } :rodata

    . = ALIGN(4096);
    .data :
    {
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.*)
        *(.data .data.*)
    } :data
    .dynamic : { *(.dynamic) } :data :dynamic
    .bss (NOLOAD) : ALIGN(16)
    {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    /DISCARD/ :
    {
        *(.comment)
        *(.note .note.*)
        *(.eh_frame .eh_frame_hdr)
        *(.interp)
    }
}
