/*
 * Pre-boot code.
 * Used only because Rust's AM considers UB any access to statics before statics
 * have been initialized. This is exactly the case for the boot code.
 * So we avoid referencing any statics in the Rust code, and delegate the
 * task to assembly piece instead.
 */

 // Load the address of a symbol into a register, PC-relative.
 //
 // The symbol must lie within +/- 4 GiB of the Program Counter.
 //
 // # Resources
 //
 // - https://sourceware.org/binutils/docs-2.36/as/AArch64_002dRelocations.html
 .macro ADR_REL register, symbol
	adrp \register, \symbol
	add  \register, \register, #:lo12:\symbol
 .endm

.section .text.chainboot.entry
/// Reset function.
///
/// Initializes the bss section before calling into the user's `main()`.
///
/// # Safety
///
/// We assume that no statics are accessed before transition to main from this function.
///
/// We are guaranteed to be in EL1 non-secure mode here.
_start:
    ADR_REL x0, __boot_core_stack_end_exclusive
    mov sp, x0

    mrs	x1, MPIDR_EL1
	and	x1, x1, {CONST_CORE_ID_MASK}
	mov	x2, {CONST_BOOT_CORE_ID}
	cmp	x1, x2
	b.ne	.L_parking_loop

    // Relocate the code from __binary_nonzero_lma to __binary_nonzero_vma
    ADR_REL x1, __binary_nonzero_lma      // Load address of the kernel binary
    ADR_REL x2, __binary_nonzero_vma      // Address to relocate to
    ADR_REL x3, __binary_nonzero_vma_end_exclusive // To calculate image size

    sub x0, x2, x0 // max loadable kernel size = VMA - SP

    // Relocate the code.
    sub x4, x3, x2 // x4 = Image size

.L__relocate_loop:
    ldp x5, x6, [x1], #16
    stp x5, x6, [x2], #16
    sub x4, x4, #16
    cbnz x4, .L__relocate_loop

    // Initialize BSS
    // Assumptions: BSS start is u64-aligned, BSS end is u128-aligned.
    // __BSS_START and __BSS_END are defined in linker script
    ADR_REL x1, __BSS_START
    ADR_REL x2, __BSS_END
.L__bss_init_loop:
    stp xzr, xzr, [x1], #16
    cmp x1, x2
    b.lt .L__bss_init_loop

    // max_kernel_size is already in x0 here
    b kernel_init

.L_parking_loop:
    wfe
    b .L_parking_loop

.size	_start, . - _start
.type	_start, function
.global	_start
