/*
 * Linker script for start-method components (`libcpu-start`): spin table, PSCI.
 *
 * Components are position-independent (`-C relocation-model=pie`, `--pie`),
 * linked at 0 and relocated by Kickstart to their physical load address: Kickstart
 * calls them at EL2 with its MMU off. Only R_AARCH64_RELATIVE relocations are
 * allowed, as for the interrupt-controller components (`linker/privileged.ld`).
 *
 * Three page-aligned loadable segments: code (R-X), read-only data plus the
 * dynamic relocation tables (R--), and writable data (RW-), which holds the
 * relocated operation table and `.bss`.
 */

ENTRY(CPU_START_OPS)

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
    /* NUL-separated device tree `enable-method` strings */
    .cpu_start_enable_method : { KEEP(*(.cpu_start_enable_method)) } :rodata

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
