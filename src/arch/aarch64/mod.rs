// mod arch::aarch64

use cortex_a::{asm, barrier, regs::*};

/// The entry to Rust, all things must be initialized
/// This is invoked from the linker script, does arch-specific init
/// and passes control to the kernel boot function kmain().
#[no_mangle]
pub unsafe extern "C" fn karch_start() -> ! {
    // Set sp to 0x80000 (just before kernel start)
    const STACK_START: u64 = 0x8_0000;

    SP.set(STACK_START);

    match read_cpu_id() {
        0 => ::kmain(),
        _ => endless_sleep(), // if not core0, indefinitely wait for events
    }
}

// Data memory barrier
#[inline]
pub fn dmb() {
    unsafe {
        barrier::dmb(barrier::SY);
    }
}

#[inline]
pub fn flushcache(address: usize) {
    unsafe {
        asm!("dc ivac, $0" :: "r"(address) :: "volatile");
    }
}

#[inline]
pub fn read_cpu_id() -> u64 {
    const CORE_MASK: u64 = 0x3;
    MPIDR_EL1.get() & CORE_MASK
}

#[inline]
pub fn current_el() -> u32 {
    CurrentEL.get()
}

#[inline]
pub fn endless_sleep() -> ! {
    loop {
        asm::wfe();
    }
}

#[inline]
pub fn loop_delay(rounds: u32) {
    for _ in 0..rounds {
        asm::nop();
    }
}

#[inline]
pub fn loop_until<F: Fn() -> bool>(f: F) {
    loop {
        if f() {
            break;
        }
        asm::nop();
    }
}
