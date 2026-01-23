// init_thread/src/boot.rs - Entry point in EL2

use core::arch::global_asm;

global_asm!(
    r#"
.section .text.boot
.global _start

_start:
    // Running in EL2 (assuming bootloader drops us here)
    // x0 = DTB pointer (from bootloader)

    // Save DTB pointer
    mov     x19, x0

    // Check we're in EL2
    mrs     x0, CurrentEL
    lsr     x0, x0, #2
    cmp     x0, #2
    b.ne    .hang           // Not EL2, hang

    // Set up EL2 stack
    adrp    x0, __stack_top
    add     x0, x0, :lo12:__stack_top
    mov     sp, x0

    // Clear BSS for init_thread
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
.clear_bss:
    cmp     x0, x1
    b.ge    .bss_done
    stp     xzr, xzr, [x0], #16
    b       .clear_bss
.bss_done:

    // Call Rust init code with DTB pointer
    mov     x0, x19
    bl      init_main

    // Should not return, but if it does...
.hang:
    wfe
    b       .hang
"#
);
