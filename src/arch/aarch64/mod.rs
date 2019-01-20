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

fn setup_paging() {
    // test if paging is enabled
    // if so, loop here

    // @todo
    // Check mmu and dcache states, loop forever on some setting

    write_ttbr_tcr_mair(
        1,
        read_translation_table_base(),
        read_translation_control(),
        read_mair(),
    );
}

struct MemMapRegion {
    virt: usize,
    phys: usize,
    size: usize,
    attr: usize,
}

impl MemMapRegion {}

// const bcm2837_mem_map: MemMapRegion[] = {
//     MemMapRegion {
//         virt: 0x00000000,
//         phys: 0x00000000,
//         size: 0x3f000000,
//         attr: PTE_BLOCK_MEMTYPE(MT_NORMAL) | PTE_BLOCK_INNER_SHARE, // mair
//     },
//     MemMapRegion {
//         virt: 0x3f000000,
//         phys: 0x3f000000,
//         size: 0x01000000,
//         attr: PTE_BLOCK_MEMTYPE(MT_DEVICE_NGNRNE) | PTE_BLOCK_NON_SHARE | PTE_BLOCK_PXN | PTE_BLOCK_UXN,
//     }
// }

pub struct BcmHost;

impl BcmHost {
    // As per https://www.raspberrypi.org/documentation/hardware/raspberrypi/peripheral_addresses.md
    /// This returns the ARM-side physical address where peripherals are mapped.
    pub fn get_peripheral_address() -> usize {
        0x3f00_0000
    }

    /// This returns the size of the peripheral's space.
    pub fn get_peripheral_size() -> usize {
        0x0100_0000
    }

    /// This returns the bus address of the SDRAM.
    pub fn get_sdram_address() -> usize {
        0xC000_0000 // uncached
    }
}
