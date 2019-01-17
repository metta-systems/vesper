// Interrupt handling


// The base address is given by VBAR_ELn and each entry has a defined offset from this
// base address. Each table has 16 entries, with each entry being 128 bytes (32 instructions)
// in size. The table effectively consists of 4 sets of 4 entries.

VBAR_EL1, VBAR_EL2, VBAR_EL3

CurrentEL with SP0: +0x0

    Synchronous
    IRQ/vIRQ
    FIQ
    SError/vSError

CurrentEL with SPx: +0x200

    Synchronous
    IRQ/vIRQ
    FIQ
    SError/vSError

Lower EL using AArch64: +0x400

    Synchronous
    IRQ/vIRQ
    FIQ
    SError/vSError

Lower EL using AArch32: +0x600

    Synchronous
    IRQ/vIRQ
    FIQ
    SError/vSError

// When the processor takes an exception to AArch64 execution state,
// all of the PSTATE interrupt masks is set automatically. This means
// that further exceptions are disabled. If software is to support
// nested exceptions, for example, to allow a higher priority interrupt
// to interrupt the handling of a lower priority source, then software needs
// to explicitly re-enable interrupts

