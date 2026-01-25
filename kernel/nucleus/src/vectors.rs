// Exception vector table for kernel

use core::arch::global_asm;

// Exception vector table must be 2KB aligned
// 16 entries × 128 bytes each = 2048 bytes total
//
// The table is organized as:
//   - 4 entries for exceptions from current EL with SP_EL0
//   - 4 entries for exceptions from current EL with SP_ELx
//   - 4 entries for exceptions from lower EL (AArch64)
//   - 4 entries for exceptions from lower EL (AArch32)

global_asm!(
    r#"
.section .vectors, "ax"
.balign 2048
.global __vectors

__vectors:
    // ═══════════════════════════════════════════════════════════════
    // Current EL with SP_EL0 (not used, we use SP_EL1)
    // ═══════════════════════════════════════════════════════════════

.balign 128
curr_el_sp0_sync:
    b       exception_handler_sync

.balign 128
curr_el_sp0_irq:
    b       exception_handler_irq

.balign 128
curr_el_sp0_fiq:
    b       exception_handler_fiq

.balign 128
curr_el_sp0_serror:
    b       exception_handler_serror

    // ═══════════════════════════════════════════════════════════════
    // Current EL with SP_ELx (kernel exceptions)
    // ═══════════════════════════════════════════════════════════════

.balign 128
curr_el_spx_sync:
    b       syscall_handler

.balign 128
curr_el_spx_irq:
    b       exception_handler_irq

.balign 128
curr_el_spx_fiq:
    b       exception_handler_fiq

.balign 128
curr_el_spx_serror:
    b       exception_handler_serror

    // ═══════════════════════════════════════════════════════════════
    // Lower EL using AArch64 (user-space syscalls and exceptions)
    // ═══════════════════════════════════════════════════════════════

.balign 128
lower_el_aarch64_sync:
    b       syscall_handler         // SVC from user space

.balign 128
lower_el_aarch64_irq:
    b       exception_handler_irq

.balign 128
lower_el_aarch64_fiq:
    b       exception_handler_fiq

.balign 128
lower_el_aarch64_serror:
    b       exception_handler_serror

    // ═══════════════════════════════════════════════════════════════
    // Lower EL using AArch32 (not supported)
    // ═══════════════════════════════════════════════════════════════

.balign 128
lower_el_aarch32_sync:
    b       exception_handler_unsupported

.balign 128
lower_el_aarch32_irq:
    b       exception_handler_unsupported

.balign 128
lower_el_aarch32_fiq:
    b       exception_handler_unsupported

.balign 128
lower_el_aarch32_serror:
    b       exception_handler_unsupported

// ═══════════════════════════════════════════════════════════════
// Exception handlers (stubs - implement properly in Rust)
// ═══════════════════════════════════════════════════════════════

exception_handler_sync:
    // Save context, call Rust handler, restore context
    b       .

exception_handler_irq:
    b       .

exception_handler_fiq:
    b       .

exception_handler_serror:
    b       .

exception_handler_unsupported:
    b       .
"#
);
