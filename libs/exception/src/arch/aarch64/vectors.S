/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

// Rust supplies these from the asserted repr(C) frame layout and origin enum.
.equ CONTEXT_SIZE, {context_size}
.equ CONTEXT_GPR, {context_gpr}
.equ CONTEXT_LR, {context_lr}
.equ CONTEXT_SPSR, {context_spsr}
.equ CONTEXT_ELR, {context_elr}
.equ CONTEXT_SP, {context_sp}
.equ CONTEXT_ORIGIN, {context_origin}
.equ CONTEXT_PADDING, {context_padding}
.equ ORIGIN_CURRENT_SP0, {origin_current_sp0}
.equ ORIGIN_CURRENT_SPX, {origin_current_spx}
.equ ORIGIN_LOWER_AARCH64, {origin_lower_aarch64}
.equ ORIGIN_LOWER_AARCH32, {origin_lower_aarch32}

.macro VECTOR_ENTRY handler
.balign 0x80
.Lvector_slot\@:
    b      .Lentry_\handler
    // Advancing to the exact slot end also rejects a slot larger than 128 bytes.
    .org   .Lvector_slot\@ + 0x80
.endm

.macro SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE handler, origin
.Lentry_\handler:
    // Exception entry selects SP_EL1 even when execution used SP_EL0.
    sub    sp,  sp,  #CONTEXT_SIZE

    stp    x0,  x1,  [sp, #CONTEXT_GPR + 16 * 0]
    stp    x2,  x3,  [sp, #CONTEXT_GPR + 16 * 1]
    stp    x4,  x5,  [sp, #CONTEXT_GPR + 16 * 2]
    stp    x6,  x7,  [sp, #CONTEXT_GPR + 16 * 3]
    stp    x8,  x9,  [sp, #CONTEXT_GPR + 16 * 4]
    stp    x10, x11, [sp, #CONTEXT_GPR + 16 * 5]
    stp    x12, x13, [sp, #CONTEXT_GPR + 16 * 6]
    stp    x14, x15, [sp, #CONTEXT_GPR + 16 * 7]
    stp    x16, x17, [sp, #CONTEXT_GPR + 16 * 8]
    stp    x18, x19, [sp, #CONTEXT_GPR + 16 * 9]
    stp    x20, x21, [sp, #CONTEXT_GPR + 16 * 10]
    stp    x22, x23, [sp, #CONTEXT_GPR + 16 * 11]
    stp    x24, x25, [sp, #CONTEXT_GPR + 16 * 12]
    stp    x26, x27, [sp, #CONTEXT_GPR + 16 * 13]
    stp    x28, x29, [sp, #CONTEXT_GPR + 16 * 14]
    str    x30,      [sp, #CONTEXT_LR]

    // All GPRs, including LR and x9, are saved before using any temporaries.
    mrs    x1,  SPSR_EL1
    mrs    x2,  ELR_EL1
    stp    x1,  x2,  [sp, #CONTEXT_SPSR]

.if \origin == ORIGIN_CURRENT_SPX
    add    x1,  sp,  #CONTEXT_SIZE
.else
    mrs    x1,  SP_EL0
.endif
    mov    x2,  #\origin
    str    x1,       [sp, #CONTEXT_SP]
    str    x2,       [sp, #CONTEXT_ORIGIN]
    str    xzr,      [sp, #CONTEXT_PADDING]

    mov    x0,  sp
    bl     \handler
    b      __restore_context
.endm

.macro FIQ_DUMMY
.balign 0x80
.Lvector_slot\@:
1:  wfe
    b      1b
    .org   .Lvector_slot\@ + 0x80
.endm

// Exception vector table must be 2KB aligned
// 16 entries × 128 bytes each = 2048 bytes total
//
// The table is organized as:
//   - 4 entries for exceptions from current EL with SP_EL0
//   - 4 entries for exceptions from current EL with SP_ELx
//   - 4 entries for exceptions from lower EL (AArch64)
//   - 4 entries for exceptions from lower EL (AArch32)
.section .vectors, "ax"
.balign 2048
.global __exception_vectors_start
__exception_vectors_start:
    // ═══════════════════════════════════════════════════════════════
    // Current EL with SP_EL0 (not used, we use SP_EL1)
    // Implementation status: trusted EL1t execution now uses these entries;
    // traps themselves still use SP_EL1.
    // ═══════════════════════════════════════════════════════════════
    VECTOR_ENTRY current_el0_synchronous                          // 0x000
    VECTOR_ENTRY current_el0_irq                                  // 0x080
    FIQ_DUMMY                                                     // 0x100
    VECTOR_ENTRY current_el0_serror                               // 0x180

    // ═══════════════════════════════════════════════════════════════
    // Current EL with SP_ELx (kernel exceptions)
    // ═══════════════════════════════════════════════════════════════
    VECTOR_ENTRY current_elx_synchronous                          // 0x200 - SVC syscall in EL1/EL2
    VECTOR_ENTRY current_elx_irq                                  // 0x280
    FIQ_DUMMY                                                     // 0x300
    VECTOR_ENTRY current_elx_serror                               // 0x380

    // ═══════════════════════════════════════════════════════════════
    // Lower EL using AArch64 (user-space syscalls and exceptions)
    // ═══════════════════════════════════════════════════════════════
    VECTOR_ENTRY lower_aarch64_synchronous                        // 0x400 - SVC syscall from user space
    VECTOR_ENTRY lower_aarch64_irq                                // 0x480
    FIQ_DUMMY                                                     // 0x500
    VECTOR_ENTRY lower_aarch64_serror                             // 0x580

    // ═══════════════════════════════════════════════════════════════
    // Lower EL using AArch32 (not supported)
    // ═══════════════════════════════════════════════════════════════
    VECTOR_ENTRY lower_aarch32_synchronous                        // 0x600
    VECTOR_ENTRY lower_aarch32_irq                                // 0x680
    FIQ_DUMMY                                                     // 0x700
    VECTOR_ENTRY lower_aarch32_serror                             // 0x780

// Out-of-line prologues keep every vector slot within its 128-byte bound.
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_synchronous, ORIGIN_CURRENT_SP0
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_irq, ORIGIN_CURRENT_SP0
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_serror, ORIGIN_CURRENT_SP0
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_synchronous, ORIGIN_CURRENT_SPX
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_irq, ORIGIN_CURRENT_SPX
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_serror, ORIGIN_CURRENT_SPX
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_synchronous, ORIGIN_LOWER_AARCH64
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_irq, ORIGIN_LOWER_AARCH64
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_serror, ORIGIN_LOWER_AARCH64
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_synchronous, ORIGIN_LOWER_AARCH32
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_irq, ORIGIN_LOWER_AARCH32
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_serror, ORIGIN_LOWER_AARCH32

.balign 0x80
.global __restore_context
__restore_context:
    ldp    x2,  x3,  [sp, #CONTEXT_SPSR]
    msr    SPSR_EL1, x2
    msr    ELR_EL1, x3

    ldr    x0,       [sp, #CONTEXT_SP]
    ldr    x1,       [sp, #CONTEXT_ORIGIN]
    cmp    x1,  #ORIGIN_CURRENT_SPX
    b.eq   1f
    // SP0/lower returns install the execution SP without repurposing SP_EL1.
    msr    SP_EL0, x0
    add    x17, sp,  #CONTEXT_SIZE
    b      2f
1:
    // SPx returns restore the saved pre-frame SP_EL1, leaving SP_EL0 alone.
    mov    x17, x0
2:
    // This frame address is a restore-only scratch value, never a continuation.
    mov    x16, sp
    ldr    x30,      [sp, #CONTEXT_LR]
    ldp    x0,  x1,  [sp, #CONTEXT_GPR + 16 * 0]
    ldp    x2,  x3,  [sp, #CONTEXT_GPR + 16 * 1]
    ldp    x4,  x5,  [sp, #CONTEXT_GPR + 16 * 2]
    ldp    x6,  x7,  [sp, #CONTEXT_GPR + 16 * 3]
    ldp    x8,  x9,  [sp, #CONTEXT_GPR + 16 * 4]
    ldp    x10, x11, [sp, #CONTEXT_GPR + 16 * 5]
    ldp    x12, x13, [sp, #CONTEXT_GPR + 16 * 6]
    ldp    x14, x15, [sp, #CONTEXT_GPR + 16 * 7]
    ldp    x18, x19, [sp, #CONTEXT_GPR + 16 * 9]
    ldp    x20, x21, [sp, #CONTEXT_GPR + 16 * 10]
    ldp    x22, x23, [sp, #CONTEXT_GPR + 16 * 11]
    ldp    x24, x25, [sp, #CONTEXT_GPR + 16 * 12]
    ldp    x26, x27, [sp, #CONTEXT_GPR + 16 * 13]
    ldp    x28, x29, [sp, #CONTEXT_GPR + 16 * 14]

    // Change SP before the last load, then recover both scratch GPRs from the
    // still-live transient frame. No restored register (including x9) is lost.
    mov    sp,  x17
    ldp    x16, x17, [x16, #CONTEXT_GPR + 16 * 8]
    eret
