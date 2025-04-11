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


.section .text.main.entry
/// Entrypoint of the processor.
///
/// Parks all cores except core0 and checks if we started in EL2/EL3. If
/// so, proceeds with setting up EL1.
///
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel main function in Rust.
///
/// Dissection of various RPi core boot stubs is available
/// [here](https://leiradel.github.io/2019/01/20/Raspberry-Pi-Stubs.html).
_boot_cores:
    mrs	x1, MPIDR_EL1
	and	x1, x1, {CONST_CORE_ID_MASK}
	mov	x2, {CONST_BOOT_CORE_ID}
	cmp	x1, x2
	b.ne	.L_parking_loop

	// Initialize BSS - prepare to fearlessly call into Rust code.
    // Assumptions: BSS start is u64-aligned, BSS end is u128-aligned.
    // __BSS_START and __BSS_END are defined in linker script
    ADR_REL x1, __BSS_START
    ADR_REL x2, __BSS_END
.L__bss_init_loop:
    stp xzr, xzr, [x1], #16
    cmp x1, x2
    b.lt .L__bss_init_loop

    ADR_REL x0, __STACK_TOP
    mov sp, x0

    b _startup_in_rust

.L_parking_loop:
    wfe
    b .L_parking_loop

.size	_boot_cores, . - _boot_cores
.type	_boot_cores, function
.global	_boot_cores
