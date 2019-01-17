// mod arch::aarch64

mod boot;

use cortex_a::{asm, barrier, regs::*};

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
