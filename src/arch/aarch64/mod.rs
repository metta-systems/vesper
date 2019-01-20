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

pub fn read_translation_table_base() -> u64 {
    let mut base: u64 = 0;
    unsafe {
        asm!("mrs $0, ttbr0_el1" : "=r"(base) ::: "volatile");
    }
    return base;
}

pub fn read_translation_control() -> u64 {
    let mut tcr: u64 = 0;
    unsafe {
        asm!("mrs $0, tcr_el1" : "=r"(tcr) ::: "volatile");
    }
    return tcr;
}

pub fn read_mair() -> u64 {
    let mut mair: u64 = 0;
    unsafe {
        asm!("mrs $0, mair_el1" : "=r"(mair) ::: "volatile");
    }
    return mair;
}

pub fn write_translation_table_base(base: usize) {
    unsafe {
        asm!("msr ttbr0_el1, $0" :: "r"(base) :: "volatile");
    }
}

// Helper function similar to u-boot
pub fn write_ttbr_tcr_mair(el: u8, base: u64, tcr: u64, attr: u64) {
    unsafe {
        asm!("dsb sy" :::: "volatile");
    }
    match (el) {
        1 => unsafe {
            asm!("msr ttbr0_el1, $0
                msr tcr_el1, $1
                msr mair_el1, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        2 => unsafe {
            asm!("msr ttbr0_el2, $0
                msr tcr_el2, $1
                msr mair_el2, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        3 => unsafe {
            asm!("msr ttbr0_el3, $0
                msr tcr_el3, $1
                msr mair_el3, $2" :: "r"(base), "r"(tcr), "r"(attr) : "memory" : "volatile");
        },
        _ => loop {},
    }
    unsafe {
        asm!("isb" :::: "volatile");
    }
}
