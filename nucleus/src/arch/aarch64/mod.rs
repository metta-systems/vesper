use {
    crate::kmain,
    cortex_a::{
        asm,
        regs::{RegisterReadOnly, RegisterReadWrite, MPIDR_EL1, SP},
    },
};

/// The entry to Rust, all things must be initialized
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel boot function kmain().
///
/// # Safety
///
/// Totally unsafe! We're in the hardware land.
///
#[no_mangle]
pub unsafe extern "C" fn karch_start() -> ! {
    // Set sp to 0x80000 (just before kernel start)
    const STACK_START: u64 = 0x8_0000;

    SP.set(STACK_START);

    match read_cpu_id() {
        0 => kmain(),
        _ => endless_sleep(), // if not core0, indefinitely wait for events
    }
}

#[inline]
pub fn read_cpu_id() -> u64 {
    const CORE_MASK: u64 = 0x3;
    MPIDR_EL1.get() & CORE_MASK
}

#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}
