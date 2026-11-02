//! A last-resort exception vector table for unit-test binaries.
//!
//! The test binaries boot without Kickstart, so `VBAR_EL1` is zero and any
//! fault (a data abort from a bad pointer, an undefined instruction) would
//! take an exception into unmapped memory and spin forever. [`install`] points
//! `VBAR_EL1` at a table whose every entry reports the exception and ends the
//! run with `semihosting::exit_failure()`, like a failed assertion does.

use {
    aarch64_cpu::{
        asm::barrier,
        registers::{ELR_EL1, ESR_EL1, FAR_EL1, Readable, VBAR_EL1, Writeable},
    },
    core::arch::global_asm,
};

// 16 entries, each at its 128-byte slot: branch to the reporting handler with
// the entry number in x0.
global_asm!(
    r#"
    .section .text.test_vectors, "ax"
    .balign 2048
    .global __test_exception_vectors
__test_exception_vectors:
    .irp entry, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
    .balign 0x80
    mov    x0, #\entry
    b      {handler}
    .endr
    "#,
    handler = sym report_exception,
);

unsafe extern "C" {
    static __test_exception_vectors: u8;
}

/// Report the exception and fail the run; never returns.
extern "C" fn report_exception(entry: u64) -> ! {
    liblog::println!("\n[failed]\n");
    liblog::println!(
        "Unhandled exception (vector entry {entry}): ESR_EL1 {:#x}, ELR_EL1 {:#x}, FAR_EL1 {:#x}",
        ESR_EL1.get(),
        ELR_EL1.get(),
        FAR_EL1.get(),
    );
    libqemu::semihosting::exit_failure()
}

/// Install the table unless a real vector table is already set (end-to-end
/// kernels booted by Kickstart have one).
pub fn install() {
    if VBAR_EL1.get() != 0 {
        return;
    }
    // The symbol is the 2 KiB-aligned table above, so its address is a valid
    // `VBAR_EL1` value.
    VBAR_EL1.set(core::ptr::addr_of!(__test_exception_vectors) as u64);
    barrier::isb(barrier::SY);
}
