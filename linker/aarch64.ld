ENTRY(karch_start)
OUTPUT_ARCH(aarch64)
/*OUTPUT_FORMAT(binary) Cannot change output format whilst linking AArch64 binaries.*/

START_ADDRESS = 0x80000; /* AArch64 boot address is 0x80000 */

SECTIONS {
    .text START_ADDRESS : AT(START_ADDRESS) {
        *(.text.karch_start)
        *(.text*)
    }

    .rodata ALIGN (4) : {
        *(.rodata*)
        FILL(0x00)
    }

    .data ALIGN (4) : {
        *(.data*)
        FILL(0x00)
    }

    .bss ALIGN (4) : {
        *(COMMON*)
        *(.bss*)
    }

    /DISCARD/ : {
        *(.comment .note* .dtors)
    }
}
