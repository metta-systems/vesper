/*
 * SPDX-License-Identifier: MIT OR BlueOak-1.0.0
 * Copyright (c) 2018-2019 Andre Richter <andre.o.richter@gmail.com>
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 * Original code distributed under MIT, additional changes are under BlueOak-1.0.0
 */

.macro SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE handler
.balign 0x80

    sub    sp,  sp,  #16 * 17

    stp    x0,  x1,  [sp, #16 * 0]
    stp    x2,  x3,  [sp, #16 * 1]
    stp    x4,  x5,  [sp, #16 * 2]
    stp    x6,  x7,  [sp, #16 * 3]
    stp    x8,  x9,  [sp, #16 * 4]
    stp    x10, x11, [sp, #16 * 5]
    stp    x12, x13, [sp, #16 * 6]
    stp    x14, x15, [sp, #16 * 7]
    stp    x16, x17, [sp, #16 * 8]
    stp    x18, x19, [sp, #16 * 9]
    stp    x20, x21, [sp, #16 * 10]
    stp    x22, x23, [sp, #16 * 11]
    stp    x24, x25, [sp, #16 * 12]
    stp    x26, x27, [sp, #16 * 13]
    stp    x28, x29, [sp, #16 * 14]

    mrs    x1,  SPSR_EL1
    mrs    x2,  ELR_EL1

    stp    x30, x1,  [sp, #16 * 15]
    str    x2,       [sp, #16 * 16]

    mov    x0,  sp
    bl     \handler
    b      __restore_context
.endm

.macro FIQ_DUMMY
.balign 0x80
1:  wfe
    b      1b
.endm

// The vector definitions
.section .vectors, "ax"
.global __exception_vectors_start
__exception_vectors_start:
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_synchronous   // 0x000
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_irq           // 0x080
    FIQ_DUMMY                                                       // 0x100
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_el0_serror        // 0x180

    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_synchronous   // 0x200
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_irq           // 0x280
    FIQ_DUMMY                                                       // 0x300
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE current_elx_serror        // 0x380

    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_synchronous // 0x400
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_irq         // 0x480
    FIQ_DUMMY                                                       // 0x500
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch64_serror      // 0x580

    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_synchronous // 0x600
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_irq         // 0x680
    FIQ_DUMMY                                                       // 0x700
    SAVE_CONTEXT_CALL_HANDLER_AND_RESTORE lower_aarch32_serror      // 0x780

.balign 0x80
.global __restore_context
__restore_context:
    ldr    x19,      [sp, #16 * 16]
    ldp    x30, x20, [sp, #16 * 15]

    msr    ELR_EL1, x19
    msr    SPSR_EL1, x20

    ldp    x0,  x1,  [sp, #16 * 0]
    ldp    x2,  x3,  [sp, #16 * 1]
    ldp    x4,  x5,  [sp, #16 * 2]
    ldp    x6,  x7,  [sp, #16 * 3]
    ldp    x8,  x9,  [sp, #16 * 4]
    ldp    x10, x11, [sp, #16 * 5]
    ldp    x12, x13, [sp, #16 * 6]
    ldp    x14, x15, [sp, #16 * 7]
    ldp    x16, x17, [sp, #16 * 8]
    ldp    x18, x19, [sp, #16 * 9]
    ldp    x20, x21, [sp, #16 * 10]
    ldp    x22, x23, [sp, #16 * 11]
    ldp    x24, x25, [sp, #16 * 12]
    ldp    x26, x27, [sp, #16 * 13]
    ldp    x28, x29, [sp, #16 * 14]

    add    sp,  sp,  #16 * 17

    eret
