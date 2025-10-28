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

// Bootloader drops us here with:
//   x0 = DTB physical address
//   x1 = kernel load address (optional, depends on bootloader)
//   MMU off, running at EL1 (or EL2 if hypervisor mode)

// #[unsafe(naked)]
// #[unsafe(no_mangle)]
// #[unsafe(link_section = ".init_thread.text.entry")]
// unsafe extern "C" fn _start() -> ! {
//     core::arch::naked_asm!(
//         // Disable interrupts
//         "msr daifset, #0xf",
//         // Get current EL
//         "mrs x2, CurrentEL",
//         "lsr x2, x2, #2",
//         "cmp x2, #2",
//         "b.eq from_el2",
//         "b setup_el1",
//         "from_el2:",
//         // Drop from EL2 to EL1 if needed
//         "mov x2, #0x3c5", // EL1h, IRQ/FIQ/Abort masked
//         "msr spsr_el2, x2",
//         "adr x2, setup_el1",
//         "msr elr_el2, x2",
//         "eret",
//         "setup_el1:",
//         // Save DTB pointer before we trash registers
//         "mov x20, x0", // x20 = DTB phys addr (callee-saved)
//         // Set up init stack (in .bss, identity mapped initially)
//         "adrp x2, __init_stack_top",
//         "add x2, x2, :lo12:__init_stack_top",
//         "mov sp, x2",
//         // Clear BSS
//         "adrp x2, __bss_start",
//         "add x2, x2, :lo12:__bss_start",
//         "adrp x3, __bss_end",
//         "add x3, x3, :lo12:__bss_end",
//         "1:",
//         "cmp x2, x3",
//         "b.ge 2f",
//         "str xzr, [x2], #8",
//         "b 1b",
//         "2:",
//         // Call Rust init with DTB pointer
//         "mov x0, x20",
//         "bl kernel_init_main",
//         // Should not return
//         "b .",
//     );
// }
